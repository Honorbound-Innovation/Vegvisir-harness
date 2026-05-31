import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";

export interface AuthSessionProfile {
  schemaVersion: "solarium.auth-session.v1";
  name: string;
  storageState: string;
  createdAt: string;
  updatedAt: string;
  scope?: unknown;
  description?: string;
  secretRefs?: string[];
  metadata: Record<string, unknown>;
}

export interface CreateAuthSessionProfileOptions {
  name: string;
  storageState: string;
  output?: string;
  scope?: unknown;
  description?: string;
  secretRefs?: string[];
  metadata?: Record<string, unknown>;
}

export interface ResolveAuthSessionOptions {
  profilePath?: string;
  storageState?: string;
  saveStorageState?: string;
  profilesDir?: string;
  name?: string;
}

export interface ResolvedAuthSession {
  storageState?: string;
  saveStorageState?: string;
  profile?: AuthSessionProfile;
}

export async function createAuthSessionProfile(options: CreateAuthSessionProfileOptions): Promise<AuthSessionProfile> {
  const now = new Date().toISOString();
  const profile: AuthSessionProfile = {
    schemaVersion: "solarium.auth-session.v1",
    name: requireNonEmpty(options.name, "name"),
    storageState: requireNonEmpty(options.storageState, "storageState"),
    createdAt: now,
    updatedAt: now,
    scope: options.scope,
    description: options.description,
    secretRefs: options.secretRefs?.map((ref, index) => requireNonEmpty(ref, `secretRefs[${index}]`)),
    metadata: options.metadata ?? {}
  };

  if (options.output) {
    await mkdir(dirname(options.output), { recursive: true });
    await writeFile(options.output, JSON.stringify(profile, null, 2), "utf8");
  }

  return profile;
}

export async function readAuthSessionProfile(path: string): Promise<AuthSessionProfile> {
  const parsed = JSON.parse(await readFile(path, "utf8")) as unknown;
  return validateAuthSessionProfile(parsed);
}

export function validateAuthSessionProfile(value: unknown): AuthSessionProfile {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Auth session profile must be a JSON object");
  }
  const candidate = value as Record<string, unknown>;
  if (candidate.schemaVersion !== "solarium.auth-session.v1") {
    throw new Error("Auth session profile schemaVersion must be solarium.auth-session.v1");
  }
  const name = requireNonEmpty(candidate.name, "name");
  const storageState = requireNonEmpty(candidate.storageState, "storageState");
  const createdAt = requireNonEmpty(candidate.createdAt, "createdAt");
  const updatedAt = requireNonEmpty(candidate.updatedAt, "updatedAt");
  if (candidate.secretRefs !== undefined && (!Array.isArray(candidate.secretRefs) || candidate.secretRefs.some((ref) => typeof ref !== "string" || !ref.trim()))) {
    throw new Error("secretRefs must be an array of non-empty secret reference strings");
  }
  if (candidate.metadata !== undefined && (!candidate.metadata || typeof candidate.metadata !== "object" || Array.isArray(candidate.metadata))) {
    throw new Error("metadata must be an object when provided");
  }

  return {
    schemaVersion: "solarium.auth-session.v1",
    name,
    storageState,
    createdAt,
    updatedAt,
    scope: candidate.scope,
    description: typeof candidate.description === "string" ? candidate.description : undefined,
    secretRefs: candidate.secretRefs as string[] | undefined,
    metadata: (candidate.metadata as Record<string, unknown> | undefined) ?? {}
  };
}

export async function resolveAuthSession(options: ResolveAuthSessionOptions): Promise<ResolvedAuthSession> {
  if (options.profilePath) {
    const profile = await readAuthSessionProfile(options.profilePath);
    return {
      profile,
      storageState: options.storageState ?? profile.storageState,
      saveStorageState: options.saveStorageState ?? profile.storageState
    };
  }

  if (options.name && options.profilesDir) {
    const profile = await readAuthSessionProfile(join(options.profilesDir, `${options.name}.auth-session.json`));
    return {
      profile,
      storageState: options.storageState ?? profile.storageState,
      saveStorageState: options.saveStorageState ?? profile.storageState
    };
  }

  return {
    storageState: options.storageState,
    saveStorageState: options.saveStorageState
  };
}

function requireNonEmpty(value: unknown, label: string): string {
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}
