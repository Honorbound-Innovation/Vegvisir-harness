import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readdir, stat, writeFile } from "node:fs/promises";
import { basename, dirname, extname, join, relative, resolve } from "node:path";
import { mkdir } from "node:fs/promises";

export interface ArtifactManifestOptions {
  roots: string[];
  output?: string;
  includeHidden?: boolean;
  maxFileBytes?: number;
  baseDir?: string;
}

export interface ArtifactManifestEntry {
  path: string;
  absolutePath: string;
  name: string;
  extension?: string;
  kind: ArtifactKind;
  sizeBytes: number;
  sha256?: string;
  skippedHash?: boolean;
  modifiedAt: string;
  createdAt: string;
}

export type ArtifactKind =
  | "screenshot"
  | "report"
  | "event-log"
  | "json-result"
  | "trace"
  | "download"
  | "storage-state"
  | "profile"
  | "config"
  | "text"
  | "other";

export interface ArtifactManifestSummary {
  files: number;
  totalBytes: number;
  byKind: Record<string, number>;
  skippedHashes: number;
}

export interface ArtifactManifest {
  generatedAt: string;
  roots: string[];
  baseDir: string;
  entries: ArtifactManifestEntry[];
  summary: ArtifactManifestSummary;
}

export async function createArtifactManifest(options: ArtifactManifestOptions): Promise<ArtifactManifest> {
  const baseDir = resolve(options.baseDir ?? process.cwd());
  const maxFileBytes = options.maxFileBytes ?? 256 * 1024 * 1024;
  const entries: ArtifactManifestEntry[] = [];

  for (const root of options.roots) {
    const absoluteRoot = resolve(baseDir, root);
    await collectEntries({
      root: absoluteRoot,
      current: absoluteRoot,
      baseDir,
      includeHidden: options.includeHidden ?? false,
      maxFileBytes,
      entries
    });
  }

  entries.sort((a, b) => a.path.localeCompare(b.path));

  const manifest: ArtifactManifest = {
    generatedAt: new Date().toISOString(),
    roots: options.roots,
    baseDir,
    entries,
    summary: summarize(entries)
  };

  if (options.output) {
    await mkdir(dirname(options.output), { recursive: true });
    await writeFile(options.output, JSON.stringify(manifest, null, 2), "utf8");
  }

  return manifest;
}

async function collectEntries(args: {
  root: string;
  current: string;
  baseDir: string;
  includeHidden: boolean;
  maxFileBytes: number;
  entries: ArtifactManifestEntry[];
}): Promise<void> {
  const info = await stat(args.current);
  const name = basename(args.current);

  if (!args.includeHidden && name.startsWith(".") && args.current !== args.root) return;

  if (info.isDirectory()) {
    const children = await readdir(args.current);
    for (const child of children) {
      await collectEntries({ ...args, current: join(args.current, child) });
    }
    return;
  }

  if (!info.isFile()) return;

  const skippedHash = info.size > args.maxFileBytes;
  args.entries.push({
    path: normalizePath(relative(args.baseDir, args.current)),
    absolutePath: args.current,
    name,
    extension: extname(name).replace(/^\./, "") || undefined,
    kind: inferArtifactKind(args.current),
    sizeBytes: info.size,
    sha256: skippedHash ? undefined : await sha256File(args.current),
    skippedHash,
    modifiedAt: info.mtime.toISOString(),
    createdAt: info.birthtime.toISOString()
  });
}

function summarize(entries: ArtifactManifestEntry[]): ArtifactManifestSummary {
  const byKind: Record<string, number> = {};
  for (const entry of entries) {
    byKind[entry.kind] = (byKind[entry.kind] ?? 0) + 1;
  }
  return {
    files: entries.length,
    totalBytes: entries.reduce((sum, entry) => sum + entry.sizeBytes, 0),
    byKind,
    skippedHashes: entries.filter((entry) => entry.skippedHash).length
  };
}

function inferArtifactKind(path: string): ArtifactKind {
  const lower = path.toLowerCase();
  const ext = extname(lower);
  const name = basename(lower);

  if ([".png", ".jpg", ".jpeg", ".webp"].includes(ext)) return "screenshot";
  if ([".md", ".html", ".htm"].includes(ext)) return "report";
  if (lower.endsWith(".events.jsonl") || ext === ".jsonl") return "event-log";
  if (ext === ".zip" && name.includes("trace")) return "trace";
  if (ext === ".json") {
    if (name.includes("state")) return "storage-state";
    if (name.includes("profile")) return "profile";
    if (name.includes("scope") || name.includes("job") || name.includes("actions")) return "config";
    return "json-result";
  }
  if ([".txt", ".csv", ".log", ".xml"].includes(ext)) return lower.includes("download") ? "download" : "text";
  if (lower.includes("download")) return "download";
  return "other";
}

async function sha256File(path: string): Promise<string> {
  const hash = createHash("sha256");
  await new Promise<void>((resolvePromise, reject) => {
    const stream = createReadStream(path);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("error", reject);
    stream.on("end", () => resolvePromise());
  });
  return hash.digest("hex");
}

function normalizePath(path: string): string {
  return path.split("\\").join("/");
}
