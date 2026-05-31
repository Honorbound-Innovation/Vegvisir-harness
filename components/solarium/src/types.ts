export type BrowserEngine = "chromium" | "firefox" | "webkit";

export type BrowserProfileName =
  | "chrome-stable"
  | "firefox-stable"
  | "safari-desktop"
  | "edge-stable"
  | "generic-desktop";

export interface BrowserProfile {
  name: BrowserProfileName | string;
  userAgent?: string;
  viewport?: {
    width: number;
    height: number;
  };
  locale?: string;
  timezoneId?: string;
  extraHTTPHeaders?: Record<string, string>;
  colorScheme?: "dark" | "light" | "no-preference";
  deviceScaleFactor?: number;
  isMobile?: boolean;
  hasTouch?: boolean;
  permissions?: string[];
}

export interface LaunchOptions {
  engine?: BrowserEngine;
  headless?: boolean;
  profile?: BrowserProfileName | BrowserProfile;
  artifactsDir?: string;
  trace?: boolean;
  /** Directory where browser downloads should be accepted and stored. */
  downloadsDir?: string;
  /** Path to a Playwright storage state JSON file to load into the browser context. */
  storageState?: string;
  /** Path where Solarium should save browser context storage state before closing. */
  saveStorageState?: string;
}

export interface NavigateOptions {
  waitUntil?: "load" | "domcontentloaded" | "networkidle" | "commit";
  timeoutMs?: number;
}

export interface ScreenshotOptions {
  path?: string;
  fullPage?: boolean;
}

export interface ExtractOptions {
  selector?: string;
  format?: "text" | "html" | "markdown";
}

export interface ExtractResult {
  url: string;
  title: string;
  content: string;
  format: "text" | "html" | "markdown";
}

export interface PageLinkObservation {
  text: string;
  href: string;
  target?: string | null;
  rel?: string | null;
}

export interface PageButtonObservation {
  text: string;
  type?: string | null;
  disabled: boolean;
  selectorHint?: string;
}

export interface PageInputObservation {
  name?: string | null;
  id?: string | null;
  type?: string | null;
  placeholder?: string | null;
  value?: string | null;
  required: boolean;
  disabled: boolean;
}

export interface PageFormObservation {
  action: string;
  method: string;
  id?: string | null;
  name?: string | null;
  fields: PageInputObservation[];
}

export interface ConsoleLogObservation {
  type: string;
  text: string;
  location?: {
    url: string;
    lineNumber: number;
    columnNumber: number;
  };
}

export interface NetworkObservation {
  url: string;
  method: string;
  resourceType: string;
  status?: number;
  ok?: boolean;
  failureText?: string;
}

export interface NetworkPolicySummary {
  allowedRequests: number;
  blockedRequests: number;
  rateLimitedRequests: number;
}

export interface PageObservation {
  observedAt: string;
  url: string;
  title: string;
  visibleText: string;
  links: PageLinkObservation[];
  buttons: PageButtonObservation[];
  inputs: PageInputObservation[];
  forms: PageFormObservation[];
  console: ConsoleLogObservation[];
  network: NetworkObservation[];
}

export interface ObservationOptions {
  maxTextChars?: number;
  maxElements?: number;
  maxConsoleEvents?: number;
  maxNetworkEvents?: number;
}


export type InspectCandidateKind = "link" | "button" | "input" | "form" | "navigation";

export interface InspectCandidate {
  kind: InspectCandidateKind;
  label: string;
  selector: string;
  roleSelector?: string;
  textSelector?: string;
  action: "click" | "fill" | "submit" | "navigate";
  href?: string;
  inputType?: string | null;
  required?: boolean;
  disabled?: boolean;
  form?: {
    selector: string;
    action: string;
    method: string;
  };
  confidence: "high" | "medium" | "low";
  reason: string;
}

export interface InspectOptions extends LaunchOptions {
  url: string;
  scope?: import("./security/scope.js").ScopePolicy;
  waitUntil?: NavigateOptions["waitUntil"];
  timeoutMs?: number;
  waitAfterNavigationMs?: number;
  observationOptions?: ObservationOptions;
  screenshotPath?: string;
  maxCandidates?: number;
  includeObservation?: boolean;
}

export interface InspectResult {
  url: string;
  finalUrl: string;
  title: string;
  inspectedAt: string;
  screenshotPath?: string;
  candidates: InspectCandidate[];
  observation?: PageObservation;
  networkPolicy?: NetworkPolicySummary;
}


export interface ActionPlanResult {
  createdAt: string;
  sourceUrl: string;
  finalUrl: string;
  title: string;
  goal?: string;
  actions: AgentAction[];
  selectedCandidates: InspectCandidate[];
  notes: string[];
}

export interface BrowseResult {
  url: string;
  title: string;
  screenshotPath?: string;
  extractedText?: string;
  observationPath?: string;
  observation?: PageObservation;
  networkPolicy?: NetworkPolicySummary;
}

export type AgentAction =
  | { type: "navigate"; url: string; waitUntil?: NavigateOptions["waitUntil"]; timeoutMs?: number }
  | { type: "click"; selector: string }
  | { type: "dblclick"; selector: string }
  | { type: "hover"; selector: string }
  | { type: "type"; selector: string; text: string }
  | { type: "press"; selector: string; key: string }
  | { type: "select"; selector: string; values: string | string[] }
  | { type: "check"; selector: string }
  | { type: "uncheck"; selector: string }
  | { type: "submit"; selector: string }
  | { type: "wait"; ms: number }
  | { type: "waitForSelector"; selector: string; state?: "attached" | "detached" | "visible" | "hidden"; timeoutMs?: number }
  | { type: "waitForUrl"; url: string; timeoutMs?: number }
  | { type: "screenshot"; path?: string; fullPage?: boolean }
  | { type: "extract"; selector?: string; format?: ExtractOptions["format"] }
  | { type: "upload"; selector: string; files: string | string[] }
  | { type: "download"; selector: string; path?: string; timeoutMs?: number }
  | { type: "observe" };

export interface DownloadResult {
  suggestedFilename: string;
  path: string;
  url?: string;
  failure?: string | null;
}

export interface AgentStepResult {
  index: number;
  action: AgentAction;
  startedAt: string;
  finishedAt: string;
  ok: boolean;
  url: string;
  title?: string;
  screenshotPath?: string;
  extracted?: ExtractResult;
  download?: DownloadResult;
  observation?: PageObservation;
  error?: string;
}

export interface AgentSessionResult {
  sessionId: string;
  startedAt: string;
  finishedAt: string;
  ok: boolean;
  steps: AgentStepResult[];
  networkPolicy?: NetworkPolicySummary;
}

export interface AgentSessionOptions extends LaunchOptions {
  sessionId?: string;
  observeAfterEachAction?: boolean;
  observationOptions?: ObservationOptions;
  evidenceDir?: string;
}


export interface LoopOptions extends LaunchOptions {
  url: string;
  scope?: import("./security/scope.js").ScopePolicy;
  loopId?: string;
  goal?: string;
  maxIterations?: number;
  actionsPerIteration?: number;
  maxCandidates?: number;
  stopAfterNoActions?: number;
  stopWhenText?: string;
  stopWhenUrl?: string;
  stopWhenSelector?: string;
  continueOnError?: boolean;
  waitUntil?: NavigateOptions["waitUntil"];
  timeoutMs?: number;
  waitAfterNavigationMs?: number;
  waitAfterActionMs?: number;
  observationOptions?: ObservationOptions;
  includeObservations?: boolean;
  evidenceDir?: string;
  screenshotEachIteration?: boolean;
  fillValue?: string;
}

export interface LoopIterationResult {
  index: number;
  startedAt: string;
  finishedAt: string;
  ok: boolean;
  url: string;
  title?: string;
  candidateCount: number;
  actions: AgentAction[];
  selectedCandidates: InspectCandidate[];
  notes: string[];
  screenshotPath?: string;
  observation?: PageObservation;
  stopReason?: string;
  error?: string;
}

export interface LoopResult {
  loopId: string;
  url: string;
  goal?: string;
  startedAt: string;
  finishedAt: string;
  ok: boolean;
  iterations: LoopIterationResult[];
  stopReason?: string;
  networkPolicy?: NetworkPolicySummary;
}

export interface CrawlOptions extends LaunchOptions {
  eventLogger?: import("./reporting/events.js").EventLogger;
  startUrl: string;
  scope: import("./security/scope.js").ScopePolicy;
  maxPages?: number;
  maxDepth?: number;
  waitUntil?: NavigateOptions["waitUntil"];
  timeoutMs?: number;
  waitAfterNavigationMs?: number;
  observationOptions?: ObservationOptions;
  includeObservations?: boolean;
  evidenceDir?: string;
  screenshots?: boolean;
}

export interface CrawlPageResult {
  url: string;
  finalUrl?: string;
  title?: string;
  depth: number;
  ok: boolean;
  discoveredLinks: PageLinkObservation[];
  forms: PageFormObservation[];
  observation?: PageObservation;
  observationPath?: string;
  screenshotPath?: string;
  startedAt: string;
  finishedAt: string;
  networkPolicy?: NetworkPolicySummary;
  error?: string;
}

export interface CrawlResult {
  startUrl: string;
  startedAt: string;
  finishedAt: string;
  ok: boolean;
  pageCount: number;
  maxPages: number;
  maxDepth: number;
  pages: CrawlPageResult[];
  networkPolicy?: NetworkPolicySummary;
}

export type AuditSeverity = "info" | "low" | "medium" | "high";
export type AuditCategory =
  | "headers"
  | "cookies"
  | "mixed-content"
  | "forms"
  | "graphql"
  | "transport"
  | "network"
  | "supply-chain"
  | "client-exposure"
  | "well-known"
  | "sensitive-file"
  | "methods"
  | "active-probe"
  | "injection-surface"
  | "authentication-surface"
  | "component-inventory"
  | "logging-monitoring"
  | "access-control"
  | "design-review"
  | "ssrf";

export interface AuditFinding {
  id: string;
  category: AuditCategory;
  severity: AuditSeverity;
  title: string;
  description: string;
  recommendation: string;
  evidence?: Record<string, unknown>;
  /** Optional audit standard associated with this finding, e.g. OWASP. */
  standard?: string;
  /** Optional OWASP mapping for report grouping and remediation context. */
  owasp?: {
    top10: string;
    asvs?: string[];
    references?: string[];
  };
}

export interface HeaderAuditFinding extends AuditFinding {
  category: "headers";
  evidence: {
    header: string;
    value: string | null;
  };
}

export interface CookieAuditFinding extends AuditFinding {
  category: "cookies";
  evidence: {
    name: string;
    domain: string;
    path: string;
    flag: string;
  };
}

export interface MixedContentAuditFinding extends AuditFinding {
  category: "mixed-content";
  evidence: {
    url: string;
    resourceType: string;
    status?: number;
  };
}

export interface AuditOptions extends LaunchOptions {
  url: string;
  scope?: import("./security/scope.js").ScopePolicy;
  waitUntil?: NavigateOptions["waitUntil"];
  timeoutMs?: number;
  waitAfterNavigationMs?: number;
  observationOptions?: ObservationOptions;
  includeObservation?: boolean;
  outputPath?: string;
}

export interface AuditResult {
  url: string;
  finalUrl?: string;
  title?: string;
  startedAt: string;
  finishedAt: string;
  ok: boolean;
  findings: AuditFinding[];
  summary: Record<AuditSeverity, number>;
  observation?: PageObservation;
  networkPolicy?: NetworkPolicySummary;
  error?: string;
}
