export { SolariumBrowser, SolariumPage } from "./browser/engine.js";
export { builtInProfiles, resolveProfile } from "./browser/profile.js";
export {
  getBuiltInProfile,
  listBuiltInProfiles,
  readBrowserProfile,
  summarizeProfile,
  validateBrowserProfile,
  type BrowserProfileSummary,
  type ProfileValidationIssue,
  type ProfileValidationResult
} from "./browser/profile-store.js";
export { browse, type BrowseOptions } from "./agent/actions.js";
export { AgentSession, runActions, type RunActionsOptions } from "./agent/session.js";
export { ObservationRecorder } from "./agent/observations.js";
export { inspectPage } from "./agent/inspect.js";
export { planActionsFromInspectResult, type PlanActionsOptions } from "./agent/plan.js";
export { crawl } from "./security/crawler.js";
export { audit } from "./security/audit.js";
export { graphqlAudit, type GraphqlAuditOptions, type GraphqlAuditResult, type GraphqlEndpointProbe, type GraphqlOperationInventory } from "./security/graphql-audit.js";
export { owaspAudit, type OwaspAuditOptions, type OwaspAuditProfile, type OwaspAuditResult, type OwaspCategorySummary } from "./security/owasp-audit.js";
export {
  renderAuditMarkdownReport,
  renderOwaspAuditMarkdownReport,
  renderGraphqlAuditMarkdownReport,
  renderCrawlMarkdownReport,
  renderLoopMarkdownReport,
  renderSessionMarkdownReport,
  type MarkdownReportOptions
} from "./reporting/markdown.js";
export {
  renderAuditHtmlReport,
  renderOwaspAuditHtmlReport,
  renderGraphqlAuditHtmlReport,
  renderCrawlHtmlReport,
  renderLoopHtmlReport,
  renderSessionHtmlReport,
  type HtmlReportOptions
} from "./reporting/html.js";
export {
  JsonlEventLogger,
  createJsonlEventLogger,
  summarizeAgentAction,
  summarizeAgentStep,
  summarizeCrawlPage,
  summarizeNetworkPolicy,
  type EventLogger,
  type EventLogEvent,
  type EventLogEventType
} from "./reporting/events.js";
export {
  readJsonlEvents,
  replayEvents,
  summarizeEvents,
  createSessionResumePlan,
  type ReplaySummary,
  type ReplaySessionSummary,
  type ReplayCrawlSummary,
  type ReplayLoopSummary,
  type ReplayErrorSummary,
  type SessionResumePlan
} from "./reporting/replay.js";
export {
  attachScopedNetworkPolicy,
  type NetworkPolicyController,
  type NetworkPolicyOptions,
  type NetworkPolicyStats
} from "./security/network-policy.js";
export { assertUrlInScope, checkUrlScope, hostMatches, validateScopePolicy, type ScopePolicy } from "./security/scope.js";
export { readSolariumJob, runJob, validateSolariumJob, type RunJobOptions, type RunJobResult, type SolariumJob, type SolariumJobMode } from "./config/job.js";
export { validateSolariumConfig, validateSolariumFile, validateActions, type SolariumValidationIssue, type SolariumValidationKind, type SolariumValidationResult } from "./config/validate.js";
export type * from "./types.js";

export { runLoop } from "./agent/loop.js";
export type { RunLoopOptions } from "./agent/loop.js";

export {
  createArtifactManifest,
  type ArtifactKind,
  type ArtifactManifest,
  type ArtifactManifestEntry,
  type ArtifactManifestOptions,
  type ArtifactManifestSummary
} from "./reporting/artifacts.js";
export {
  createEvidenceRunManifest,
  summarizeEvidenceAction,
  type EvidenceActionSummary,
  type EvidenceManifestOptions,
  type EvidenceRunKind,
  type EvidenceRunManifest
} from "./reporting/evidence.js";

export {
  runJsonRpcServer,
  handleJsonRpcRequest,
  type JsonRpcRequest,
  type JsonRpcServerOptions
} from "./server/json-rpc.js";
export {
  SolariumJsonRpcClient,
  createSolariumJsonRpcClient,
  launchSolariumServer,
  type JsonRpcClientOptions,
  type JsonRpcResponse,
  type JsonRpcResponseError,
  type LaunchedSolariumServerClient,
  type LaunchSolariumServerOptions,
  type SolariumInitializeResult,
  type SolariumToolCallResult,
  type SolariumToolDefinition,
  type SolariumToolsListResult
} from "./client/json-rpc.js";

export {
  createWorkflowSeedFromFiles,
  createWorkflowSeedMarkdown,
  renderWorkflowSeedMarkdown,
  type WorkflowSeedFromFilesOptions,
  type WorkflowSeedInput,
  type WorkflowSeedOptions
} from "./skills/workflow-seed.js";

export {
  createAuthSessionProfile,
  readAuthSessionProfile,
  resolveAuthSession,
  validateAuthSessionProfile,
  type AuthSessionProfile,
  type CreateAuthSessionProfileOptions,
  type ResolveAuthSessionOptions,
  type ResolvedAuthSession
} from "./browser/auth-session.js";
