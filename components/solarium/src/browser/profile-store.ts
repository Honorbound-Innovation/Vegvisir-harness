import { readFile } from "node:fs/promises";
import type { BrowserProfile, BrowserProfileName } from "../types.js";
import { builtInProfiles } from "./profile.js";

export interface BrowserProfileSummary {
  name: string;
  builtIn: boolean;
  userAgent?: string;
  viewport?: BrowserProfile["viewport"];
  locale?: string;
  timezoneId?: string;
  colorScheme?: BrowserProfile["colorScheme"];
  deviceScaleFactor?: number;
  isMobile?: boolean;
  hasTouch?: boolean;
  extraHTTPHeaderNames?: string[];
  permissions?: string[];
}

export interface ProfileValidationIssue {
  path: string;
  message: string;
}

export interface ProfileValidationResult {
  ok: boolean;
  issues: ProfileValidationIssue[];
}

export function listBuiltInProfiles(): BrowserProfileSummary[] {
  return Object.values(builtInProfiles).map((profile) => summarizeProfile(profile, true));
}

export function getBuiltInProfile(name: string): BrowserProfile | undefined {
  return builtInProfiles[name as BrowserProfileName];
}

export function summarizeProfile(profile: BrowserProfile, builtIn = false): BrowserProfileSummary {
  return {
    name: profile.name,
    builtIn,
    userAgent: profile.userAgent,
    viewport: profile.viewport,
    locale: profile.locale,
    timezoneId: profile.timezoneId,
    colorScheme: profile.colorScheme,
    deviceScaleFactor: profile.deviceScaleFactor,
    isMobile: profile.isMobile,
    hasTouch: profile.hasTouch,
    extraHTTPHeaderNames: profile.extraHTTPHeaders ? Object.keys(profile.extraHTTPHeaders).sort() : undefined,
    permissions: profile.permissions
  };
}

export async function readBrowserProfile(path: string): Promise<BrowserProfile> {
  const raw = await readFile(path, "utf8");
  const parsed = JSON.parse(raw) as unknown;
  const validation = validateBrowserProfile(parsed);
  if (!validation.ok) {
    const details = validation.issues.map((issue) => `${issue.path}: ${issue.message}`).join("; ");
    throw new Error(`Invalid browser profile ${path}: ${details}`);
  }
  return parsed as BrowserProfile;
}

export function validateBrowserProfile(value: unknown): ProfileValidationResult {
  const issues: ProfileValidationIssue[] = [];
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { ok: false, issues: [{ path: "$", message: "Profile must be a JSON object" }] };
  }

  const profile = value as Record<string, unknown>;
  if (typeof profile.name !== "string" || !profile.name.trim()) {
    issues.push({ path: "$.name", message: "Profile name must be a non-empty string" });
  }

  optionalString(profile.userAgent, "$.userAgent", issues);
  optionalString(profile.locale, "$.locale", issues);
  optionalString(profile.timezoneId, "$.timezoneId", issues);

  if (profile.colorScheme !== undefined && !["dark", "light", "no-preference"].includes(String(profile.colorScheme))) {
    issues.push({ path: "$.colorScheme", message: "colorScheme must be dark, light, or no-preference" });
  }

  optionalBoolean(profile.isMobile, "$.isMobile", issues);
  optionalBoolean(profile.hasTouch, "$.hasTouch", issues);
  optionalPositiveNumber(profile.deviceScaleFactor, "$.deviceScaleFactor", issues, false);

  if (profile.viewport !== undefined) {
    if (!profile.viewport || typeof profile.viewport !== "object" || Array.isArray(profile.viewport)) {
      issues.push({ path: "$.viewport", message: "viewport must be an object" });
    } else {
      const viewport = profile.viewport as Record<string, unknown>;
      optionalPositiveNumber(viewport.width, "$.viewport.width", issues, true);
      optionalPositiveNumber(viewport.height, "$.viewport.height", issues, true);
      if (viewport.width === undefined) issues.push({ path: "$.viewport.width", message: "viewport.width is required" });
      if (viewport.height === undefined) issues.push({ path: "$.viewport.height", message: "viewport.height is required" });
    }
  }

  if (profile.extraHTTPHeaders !== undefined) {
    if (!profile.extraHTTPHeaders || typeof profile.extraHTTPHeaders !== "object" || Array.isArray(profile.extraHTTPHeaders)) {
      issues.push({ path: "$.extraHTTPHeaders", message: "extraHTTPHeaders must be an object" });
    } else {
      for (const [key, headerValue] of Object.entries(profile.extraHTTPHeaders as Record<string, unknown>)) {
        if (!key.trim()) issues.push({ path: "$.extraHTTPHeaders", message: "header names must be non-empty" });
        if (typeof headerValue !== "string") {
          issues.push({ path: `$.extraHTTPHeaders.${key}`, message: "header values must be strings" });
        }
      }
    }
  }

  if (profile.permissions !== undefined) {
    if (!Array.isArray(profile.permissions) || profile.permissions.some((permission) => typeof permission !== "string" || !permission.trim())) {
      issues.push({ path: "$.permissions", message: "permissions must be an array of non-empty strings" });
    }
  }

  return { ok: issues.length === 0, issues };
}

function optionalString(value: unknown, path: string, issues: ProfileValidationIssue[]): void {
  if (value !== undefined && typeof value !== "string") {
    issues.push({ path, message: "must be a string" });
  }
}

function optionalBoolean(value: unknown, path: string, issues: ProfileValidationIssue[]): void {
  if (value !== undefined && typeof value !== "boolean") {
    issues.push({ path, message: "must be a boolean" });
  }
}

function optionalPositiveNumber(value: unknown, path: string, issues: ProfileValidationIssue[], integer: boolean): void {
  if (value === undefined) return;
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0 || (integer && !Number.isInteger(value))) {
    issues.push({ path, message: integer ? "must be a positive integer" : "must be a positive number" });
  }
}
