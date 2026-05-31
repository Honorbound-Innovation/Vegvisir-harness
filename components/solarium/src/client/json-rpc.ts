import { spawn, type ChildProcessWithoutNullStreams, type SpawnOptionsWithoutStdio } from "node:child_process";
import { createInterface, type Interface as ReadlineInterface } from "node:readline";
import type { Readable, Writable } from "node:stream";
import type { JsonRpcRequest } from "../server/json-rpc.js";

export interface JsonRpcResponse<T = unknown> {
  jsonrpc: "2.0";
  id: string | number | null;
  result?: T;
  error?: JsonRpcResponseError;
}

export interface JsonRpcResponseError {
  code: number;
  message: string;
  data?: unknown;
}

export interface JsonRpcClientOptions {
  input: Readable;
  output: Writable;
  requestTimeoutMs?: number;
}

export interface SolariumToolDefinition {
  name: string;
  description?: string;
  inputSchema?: Record<string, unknown>;
}

export interface SolariumInitializeResult {
  protocolVersion?: string;
  serverInfo?: {
    name?: string;
    version?: string;
  };
  capabilities?: Record<string, unknown>;
  instructions?: string;
}

export interface SolariumToolsListResult {
  tools: SolariumToolDefinition[];
}

export interface SolariumToolCallResult<T = unknown> {
  content?: Array<{ type: string; text?: string; [key: string]: unknown }>;
  structuredContent?: T;
  [key: string]: unknown;
}

export interface LaunchSolariumServerOptions {
  /** Command to execute. Defaults to `solarium`; for local development use `npm run dev -- server` externally or pass a command/args pair. */
  command?: string;
  /** Arguments for the command. Defaults to `["server"]`. */
  args?: string[];
  cwd?: string;
  env?: NodeJS.ProcessEnv;
  requestTimeoutMs?: number;
  /** Pipe server stderr to the parent process stderr. Defaults to false. */
  inheritStderr?: boolean;
  spawnOptions?: Omit<SpawnOptionsWithoutStdio, "cwd" | "env">;
}

export interface LaunchedSolariumServerClient {
  client: SolariumJsonRpcClient;
  process: ChildProcessWithoutNullStreams;
  close(): Promise<void>;
}

type PendingRequest = {
  resolve: (value: unknown) => void;
  reject: (reason?: unknown) => void;
  timer?: NodeJS.Timeout;
};

const DEFAULT_TIMEOUT_MS = 30_000;

/**
 * Minimal newline-delimited JSON-RPC 2.0 client for Solarium's stdio server.
 *
 * This is intentionally transport-small: Vegvisir, MCP bridges, and other agents
 * can bind it to any readable/writable pair, including a spawned Solarium server
 * process or a managed subprocess transport.
 */
export class SolariumJsonRpcClient {
  private nextId = 1;
  private readonly pending = new Map<string | number, PendingRequest>();
  private readonly rl: ReadlineInterface;
  private closed = false;
  private readonly requestTimeoutMs: number;

  constructor(private readonly options: JsonRpcClientOptions) {
    this.requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_TIMEOUT_MS;
    this.rl = createInterface({ input: options.input, crlfDelay: Infinity });
    this.rl.on("line", (line) => this.handleLine(line));
    this.rl.on("close", () => this.rejectAll(new Error("JSON-RPC input closed")));
    options.input.on("error", (error) => this.rejectAll(error));
    options.output.on("error", (error) => this.rejectAll(error));
  }

  async initialize(params: Record<string, unknown> = {}): Promise<SolariumInitializeResult> {
    return this.call<SolariumInitializeResult>("initialize", params);
  }

  initialized(): void {
    this.notify("notifications/initialized", {});
  }

  async ping(): Promise<unknown> {
    return this.call("ping", {});
  }

  async listTools(): Promise<SolariumToolDefinition[]> {
    const result = await this.call<SolariumToolsListResult>("tools/list", {});
    return result.tools;
  }

  async callTool<T = unknown>(name: string, args: Record<string, unknown> = {}): Promise<SolariumToolCallResult<T>> {
    return this.call<SolariumToolCallResult<T>>("tools/call", { name, arguments: args });
  }

  async callSolariumTool<T = unknown>(name: string, args: Record<string, unknown> = {}): Promise<T> {
    return this.call<T>(name, args);
  }

  async call<T = unknown>(method: string, params?: unknown): Promise<T> {
    if (this.closed) throw new Error("JSON-RPC client is closed");
    const id = this.nextId++;
    const request: JsonRpcRequest = { jsonrpc: "2.0", id, method, ...(params === undefined ? {} : { params }) };

    const promise = new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`JSON-RPC request timed out: ${method}`));
      }, this.requestTimeoutMs);
      this.pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
    });

    this.options.output.write(`${JSON.stringify(request)}\n`);
    return promise;
  }

  notify(method: string, params?: unknown): void {
    if (this.closed) throw new Error("JSON-RPC client is closed");
    const request: JsonRpcRequest = { jsonrpc: "2.0", method, ...(params === undefined ? {} : { params }) };
    this.options.output.write(`${JSON.stringify(request)}\n`);
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.rl.close();
    this.options.output.end();
    this.rejectAll(new Error("JSON-RPC client closed"));
  }

  private handleLine(line: string): void {
    const trimmed = line.trim();
    if (!trimmed) return;

    let response: JsonRpcResponse;
    try {
      response = JSON.parse(trimmed) as JsonRpcResponse;
    } catch (error) {
      this.rejectAll(new Error(`Invalid JSON-RPC response: ${error instanceof Error ? error.message : String(error)}`));
      return;
    }

    if (response.id === null || response.id === undefined) return;
    const pending = this.pending.get(response.id);
    if (!pending) return;
    this.pending.delete(response.id);
    if (pending.timer) clearTimeout(pending.timer);

    if (response.error) {
      const error = new Error(response.error.message) as Error & { code?: number; data?: unknown };
      error.code = response.error.code;
      error.data = response.error.data;
      pending.reject(error);
      return;
    }

    pending.resolve(response.result);
  }

  private rejectAll(error: unknown): void {
    for (const [id, pending] of this.pending) {
      if (pending.timer) clearTimeout(pending.timer);
      pending.reject(error);
      this.pending.delete(id);
    }
  }
}

export function createSolariumJsonRpcClient(options: JsonRpcClientOptions): SolariumJsonRpcClient {
  return new SolariumJsonRpcClient(options);
}

export function launchSolariumServer(options: LaunchSolariumServerOptions = {}): LaunchedSolariumServerClient {
  const command = options.command ?? "solarium";
  const args = options.args ?? ["server"];
  const child = spawn(command, args, {
    cwd: options.cwd,
    env: options.env,
    stdio: "pipe",
    ...options.spawnOptions
  });

  if (options.inheritStderr) {
    child.stderr.pipe(process.stderr);
  }

  const client = new SolariumJsonRpcClient({
    input: child.stdout,
    output: child.stdin,
    requestTimeoutMs: options.requestTimeoutMs
  });

  child.on("error", (error) => {
    // Surface spawn failures to any in-flight request. Future calls fail because stdio closes.
    child.stderr.emit("error", error);
  });

  return {
    client,
    process: child,
    close: () => closeLaunchedSolariumServer(client, child)
  };
}

async function closeLaunchedSolariumServer(client: SolariumJsonRpcClient, child: ChildProcessWithoutNullStreams): Promise<void> {
  client.close();
  if (child.exitCode !== null || child.signalCode !== null) return;

  await new Promise<void>((resolve) => {
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      resolve();
    }, 2_000);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
    child.kill("SIGTERM");
  });
}
