export interface ScopePolicy {
  allowedHosts?: string[];
  blockedHosts?: string[];
  maxRequestsPerMinute?: number;
  authorizationNote?: string;
}

export interface ScopeDecision {
  allowed: boolean;
  reason: string;
  url: string;
  host?: string;
}

export function assertUrlInScope(url: string, scope?: ScopePolicy): void {
  const decision = checkUrlScope(url, scope);
  if (!decision.allowed) {
    throw new Error(decision.reason);
  }
}

export function checkUrlScope(url: string, scope?: ScopePolicy): ScopeDecision {
  if (!scope) {
    return { allowed: true, reason: "No scope policy configured", url };
  }

  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return { allowed: false, reason: `Invalid URL: ${url}`, url };
  }

  if (!["http:", "https:"].includes(parsed.protocol)) {
    return {
      allowed: false,
      reason: `URL scheme is not allowed by scope policy: ${parsed.protocol}`,
      url,
      host: parsed.hostname
    };
  }

  const host = parsed.hostname;

  if (scope.blockedHosts?.some((pattern) => hostMatches(host, pattern))) {
    return {
      allowed: false,
      reason: `URL is blocked by scope policy: ${url}`,
      url,
      host
    };
  }

  if (scope.allowedHosts?.length && !scope.allowedHosts.some((pattern) => hostMatches(host, pattern))) {
    return {
      allowed: false,
      reason: `URL is outside allowed scope: ${url}`,
      url,
      host
    };
  }

  return {
    allowed: true,
    reason: "URL is within scope policy",
    url,
    host
  };
}

export function validateScopePolicy(scope: unknown): ScopePolicy {
  if (!scope || typeof scope !== "object" || Array.isArray(scope)) {
    throw new Error("Scope policy must be a JSON object");
  }

  const candidate = scope as Record<string, unknown>;
  const policy: ScopePolicy = {};

  if (candidate.allowedHosts !== undefined) {
    policy.allowedHosts = validateHostPatternArray(candidate.allowedHosts, "allowedHosts");
  }

  if (candidate.blockedHosts !== undefined) {
    policy.blockedHosts = validateHostPatternArray(candidate.blockedHosts, "blockedHosts");
  }

  if (candidate.maxRequestsPerMinute !== undefined) {
    if (
      typeof candidate.maxRequestsPerMinute !== "number" ||
      !Number.isFinite(candidate.maxRequestsPerMinute) ||
      candidate.maxRequestsPerMinute <= 0
    ) {
      throw new Error("maxRequestsPerMinute must be a positive number");
    }
    policy.maxRequestsPerMinute = candidate.maxRequestsPerMinute;
  }

  if (candidate.authorizationNote !== undefined) {
    if (typeof candidate.authorizationNote !== "string" || !candidate.authorizationNote.trim()) {
      throw new Error("authorizationNote must be a non-empty string when provided");
    }
    policy.authorizationNote = candidate.authorizationNote;
  }

  if (!policy.allowedHosts?.length && !policy.blockedHosts?.length) {
    throw new Error("Scope policy must define allowedHosts and/or blockedHosts");
  }

  return policy;
}

export function hostMatches(host: string, pattern: string): boolean {
  const normalizedHost = host.toLowerCase();
  const normalizedPattern = pattern.toLowerCase();

  if (normalizedPattern.startsWith("*.")) {
    const suffix = normalizedPattern.slice(1);
    return normalizedHost.endsWith(suffix) && normalizedHost !== normalizedPattern.slice(2);
  }

  return normalizedHost === normalizedPattern;
}

function validateHostPatternArray(value: unknown, label: string): string[] {
  if (!Array.isArray(value)) {
    throw new Error(`${label} must be an array of host patterns`);
  }

  return value.map((entry, index) => {
    if (typeof entry !== "string" || !entry.trim()) {
      throw new Error(`${label}[${index}] must be a non-empty string`);
    }

    const normalized = entry.trim().toLowerCase();
    if (normalized.includes("://") || normalized.includes("/")) {
      throw new Error(`${label}[${index}] must be a host pattern, not a full URL: ${entry}`);
    }

    return normalized;
  });
}
