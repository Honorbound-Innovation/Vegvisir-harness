import { createHash, createHmac } from "node:crypto";

export type JsrtFrameType =
  | "SessionCreate"
  | "SessionInit"
  | "StateDeclare"
  | "StateTransition"
  | "PromptDispatch"
  | "PromptResult"
  | "ContractBind"
  | "ValidationRequest"
  | "ValidationResult"
  | "ArtifactDeclare"
  | "ArtifactSubmit"
  | "ArtifactAccept"
  | "ArtifactReject"
  | "RemediationRequest"
  | "RemediationResult"
  | "ToolRequest"
  | "ToolResult"
  | "CheckpointCreate"
  | "CheckpointRestore"
  | "Error"
  | "Complete"
  | "Terminate"
  | "Heartbeat"
  | "CapabilityDeclare";

export type JsrtSessionState =
  | "Created"
  | "Initialized"
  | "Ready"
  | "Active"
  | "AwaitingInput"
  | "AwaitingValidation"
  | "AwaitingRemediation"
  | "Paused"
  | "Checkpointed"
  | "Completed"
  | "Rejected"
  | "Failed"
  | "Terminated"
  | "Expired";

export interface JsrtFrame {
  jsrt_version: string;
  frame_id: string;
  session_id: string;
  sequence: number;
  frame_type: string;
  timestamp: string;
  state: JsrtSessionState;
  payload: Record<string, unknown>;
  stream_id?: string;
  correlation_id?: string;
  parent_frame_id?: string;
  profile?: string;
  source?: string;
  target?: string;
  checksum?: string;
  signature?: string;
  tags?: string[];
  priority?: number;
  contract_refs?: string[];
  prompt_refs?: string[];
  checkpoint?: Record<string, unknown>;
  expires_at?: string;
  retry_of?: string;
  supersedes?: string;
  annotations?: Record<string, unknown>;
  trace?: Record<string, unknown>;
  diagnostics?: Record<string, unknown>;
  capabilities?: string[];
  [key: string]: unknown;
}

export interface JsrtIssue {
  code:
    | "SyntaxError"
    | "SchemaError"
    | "MissingField"
    | "TypeMismatch"
    | "UnknownFrameType"
    | "InvalidSequence"
    | "UnknownSession"
    | "IllegalTransition"
    | "ReferenceResolutionError"
    | "ProfileViolation"
    | "ContractViolation"
    | "PayloadOverflow"
    | "SignatureFailure"
    | "ChecksumFailure"
    | "ReplayDetected"
    | "Timeout"
    | "RuntimeFailure";
  message: string;
  frame_id?: string;
  sequence?: number;
}

export interface JsrtValidationResult {
  valid: boolean;
  issues: JsrtIssue[];
}

export interface JsrtCheckpointRecord {
  checkpoint_id: string;
  frame_id: string;
  sequence: number;
  state: JsrtSessionState;
  profile: string;
  integrity_marker: string;
  active_contract_refs: string[];
}

export interface JsrtSessionSnapshot {
  session_id: string;
  profile: string;
  state: JsrtSessionState;
  last_sequence: number;
  frame_count: number;
  accepted_frame_ids: string[];
  pending_frame_ids: string[];
  capabilities: string[];
  checkpoints: JsrtCheckpointRecord[];
  active_contract_refs: string[];
  blocking_validation_targets: string[];
  pending_remediation_targets: string[];
}

export interface JsrtApplyResult {
  snapshot?: JsrtSessionSnapshot;
  accepted: string[];
  pending: string[];
  rejected: string[];
  issues: JsrtIssue[];
}

export interface JsrtDocument {
  profile?: string;
  prompt_registry?: string[];
  contract_registry?: string[];
  frames: JsrtFrame[];
}

export interface JsrtProfile {
  name: string;
  allowed_frame_types: Set<string>;
  allow_custom_frame_types: boolean;
  unknown_field_policy: "reject" | "allow";
  max_payload_bytes: number;
  max_frame_bytes: number;
  max_sequence_gap: number;
  require_checksum: boolean;
  require_signature: boolean;
  sequence_mode: "strict" | "hold_pending";
  replay_policy: "reject" | "allow_retry";
  allow_reopen_terminal: boolean;
}

export interface JsrtContext {
  profile?: string;
  prompt_registry?: string[];
  contract_registry?: string[];
  hmac_secret?: string;
  now?: Date;
}

export interface ParsedJsrtDocument {
  document: JsrtDocument;
  frames: JsrtFrame[];
  issues: JsrtIssue[];
}

const CORE_FRAME_TYPES = new Set<JsrtFrameType>([
  "SessionCreate",
  "SessionInit",
  "StateDeclare",
  "StateTransition",
  "PromptDispatch",
  "PromptResult",
  "ContractBind",
  "ValidationRequest",
  "ValidationResult",
  "ArtifactDeclare",
  "ArtifactSubmit",
  "ArtifactAccept",
  "ArtifactReject",
  "RemediationRequest",
  "RemediationResult",
  "ToolRequest",
  "ToolResult",
  "CheckpointCreate",
  "CheckpointRestore",
  "Error",
  "Complete",
  "Terminate",
  "Heartbeat",
  "CapabilityDeclare",
]);

const SESSION_STATES = new Set<JsrtSessionState>([
  "Created",
  "Initialized",
  "Ready",
  "Active",
  "AwaitingInput",
  "AwaitingValidation",
  "AwaitingRemediation",
  "Paused",
  "Checkpointed",
  "Completed",
  "Rejected",
  "Failed",
  "Terminated",
  "Expired",
]);

const ERROR_SEVERITIES = new Set(["Info", "Warning", "Error", "Critical", "Blocking"]);
const ERROR_RECOVERABILITY = new Set(["none", "retryable", "remediable", "resumable", "manual"]);

const REQUIRED_FIELDS: Array<keyof JsrtFrame> = [
  "jsrt_version",
  "frame_id",
  "session_id",
  "sequence",
  "frame_type",
  "timestamp",
  "state",
  "payload",
];

const RESERVED_FIELDS = new Set([
  "jsrt_version",
  "frame_id",
  "session_id",
  "sequence",
  "frame_type",
  "timestamp",
  "state",
  "payload",
  "stream_id",
  "correlation_id",
  "parent_frame_id",
  "profile",
  "source",
  "target",
  "checksum",
  "signature",
  "tags",
  "priority",
  "contract_refs",
  "prompt_refs",
  "checkpoint",
  "expires_at",
  "retry_of",
  "supersedes",
  "annotations",
  "trace",
  "diagnostics",
  "capabilities",
]);

const TRANSITIONS: Record<JsrtSessionState, Set<JsrtSessionState>> = {
  Created: new Set(["Initialized", "Terminated", "Failed", "Expired"]),
  Initialized: new Set(["Ready", "Terminated", "Failed", "Expired"]),
  Ready: new Set(["Active", "Paused", "Terminated", "Failed", "Expired"]),
  Active: new Set([
    "AwaitingInput",
    "AwaitingValidation",
    "AwaitingRemediation",
    "Paused",
    "Checkpointed",
    "Completed",
    "Failed",
    "Rejected",
    "Terminated",
    "Expired",
    "Active",
  ]),
  AwaitingInput: new Set(["Active", "Paused", "Failed", "Terminated", "Expired"]),
  AwaitingValidation: new Set(["Active", "AwaitingRemediation", "Failed", "Rejected", "Terminated", "Expired"]),
  AwaitingRemediation: new Set(["Active", "Failed", "Rejected", "Terminated", "Expired"]),
  Paused: new Set(["Active", "Checkpointed", "Failed", "Terminated", "Expired"]),
  Checkpointed: new Set(["Active", "Paused", "Failed", "Terminated", "Expired"]),
  Completed: new Set(["Completed"]),
  Rejected: new Set(["Rejected"]),
  Failed: new Set(["Failed"]),
  Terminated: new Set(["Terminated"]),
  Expired: new Set(["Expired"]),
};

const TERMINAL_STATES = new Set<JsrtSessionState>(["Completed", "Rejected", "Failed", "Terminated", "Expired"]);

const FRAME_STATE_BY_TYPE: Partial<Record<JsrtFrameType, JsrtSessionState>> = {
  SessionCreate: "Created",
  SessionInit: "Initialized",
  Complete: "Completed",
  Terminate: "Terminated",
};

const FRAME_PAYLOAD_REQUIRED: Partial<Record<JsrtFrameType, string[]>> = {
  SessionCreate: ["purpose"],
  PromptDispatch: ["prompt_id"],
  PromptResult: ["prompt_id", "status"],
  ContractBind: ["contract_id", "target_ref"],
  ValidationRequest: ["target_ref"],
  ValidationResult: ["target_ref", "passed"],
  ArtifactDeclare: ["artifact_id"],
  ArtifactSubmit: ["artifact_id"],
  ArtifactAccept: ["artifact_id"],
  ArtifactReject: ["artifact_id", "reasons"],
  RemediationRequest: ["target_ref"],
  RemediationResult: ["target_ref", "status"],
  ToolRequest: ["tool"],
  ToolResult: ["tool", "status"],
  CheckpointCreate: ["checkpoint_id"],
  CheckpointRestore: ["checkpoint_id", "integrity_hash"],
  Error: ["error_id", "category", "message", "severity", "recoverability", "suggested_remediation", "timestamp"],
  CapabilityDeclare: ["capabilities"],
};

const PROFILES: Record<string, JsrtProfile> = {
  StrictDeterministic: {
    name: "StrictDeterministic",
    allowed_frame_types: new Set(CORE_FRAME_TYPES),
    allow_custom_frame_types: false,
    unknown_field_policy: "reject",
    max_payload_bytes: 64 * 1024,
    max_frame_bytes: 96 * 1024,
    max_sequence_gap: 0,
    require_checksum: false,
    require_signature: false,
    sequence_mode: "strict",
    replay_policy: "reject",
    allow_reopen_terminal: false,
  },
  Development: {
    name: "Development",
    allowed_frame_types: new Set(CORE_FRAME_TYPES),
    allow_custom_frame_types: true,
    unknown_field_policy: "allow",
    max_payload_bytes: 256 * 1024,
    max_frame_bytes: 384 * 1024,
    max_sequence_gap: 256,
    require_checksum: false,
    require_signature: false,
    sequence_mode: "hold_pending",
    replay_policy: "allow_retry",
    allow_reopen_terminal: true,
  },
  HighSecurity: {
    name: "HighSecurity",
    allowed_frame_types: new Set(CORE_FRAME_TYPES),
    allow_custom_frame_types: false,
    unknown_field_policy: "reject",
    max_payload_bytes: 64 * 1024,
    max_frame_bytes: 96 * 1024,
    max_sequence_gap: 0,
    require_checksum: true,
    require_signature: true,
    sequence_mode: "strict",
    replay_policy: "reject",
    allow_reopen_terminal: false,
  },
  OfflineBatch: {
    name: "OfflineBatch",
    allowed_frame_types: new Set(CORE_FRAME_TYPES),
    allow_custom_frame_types: false,
    unknown_field_policy: "reject",
    max_payload_bytes: 512 * 1024,
    max_frame_bytes: 768 * 1024,
    max_sequence_gap: 0,
    require_checksum: true,
    require_signature: false,
    sequence_mode: "strict",
    replay_policy: "reject",
    allow_reopen_terminal: false,
  },
  LowToken: {
    name: "LowToken",
    allowed_frame_types: new Set(CORE_FRAME_TYPES),
    allow_custom_frame_types: false,
    unknown_field_policy: "reject",
    max_payload_bytes: 24 * 1024,
    max_frame_bytes: 32 * 1024,
    max_sequence_gap: 0,
    require_checksum: false,
    require_signature: false,
    sequence_mode: "strict",
    replay_policy: "reject",
    allow_reopen_terminal: false,
  },
  MultiAgent: {
    name: "MultiAgent",
    allowed_frame_types: new Set(CORE_FRAME_TYPES),
    allow_custom_frame_types: true,
    unknown_field_policy: "allow",
    max_payload_bytes: 256 * 1024,
    max_frame_bytes: 512 * 1024,
    max_sequence_gap: 128,
    require_checksum: true,
    require_signature: false,
    sequence_mode: "hold_pending",
    replay_policy: "allow_retry",
    allow_reopen_terminal: true,
  },
  ArtifactHeavy: {
    name: "ArtifactHeavy",
    allowed_frame_types: new Set(CORE_FRAME_TYPES),
    allow_custom_frame_types: false,
    unknown_field_policy: "reject",
    max_payload_bytes: 1024 * 1024,
    max_frame_bytes: 1500 * 1024,
    max_sequence_gap: 32,
    require_checksum: true,
    require_signature: false,
    sequence_mode: "hold_pending",
    replay_policy: "allow_retry",
    allow_reopen_terminal: false,
  },
  ValidationFirst: {
    name: "ValidationFirst",
    allowed_frame_types: new Set(CORE_FRAME_TYPES),
    allow_custom_frame_types: false,
    unknown_field_policy: "reject",
    max_payload_bytes: 96 * 1024,
    max_frame_bytes: 140 * 1024,
    max_sequence_gap: 0,
    require_checksum: true,
    require_signature: false,
    sequence_mode: "strict",
    replay_policy: "reject",
    allow_reopen_terminal: false,
  },
};

export function listJsrtProfiles(): string[] {
  return Object.keys(PROFILES);
}

export function getJsrtProfile(name?: string): JsrtProfile {
  const profile = PROFILES[name ?? "StrictDeterministic"] ?? PROFILES.StrictDeterministic;
  return {
    ...profile,
    allowed_frame_types: new Set(profile.allowed_frame_types),
  };
}

function profileFor(name?: string): JsrtProfile {
  return getJsrtProfile(name);
}

function isIsoDate(value: unknown): boolean {
  if (typeof value !== "string") return false;
  const d = new Date(value);
  return !Number.isNaN(d.getTime());
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function canonicalFrameForHash(frame: JsrtFrame): string {
  const clone: Record<string, unknown> = {};
  const keys = Object.keys(frame)
    .filter((k) => k !== "checksum" && k !== "signature")
    .sort();
  for (const key of keys) {
    clone[key] = (frame as Record<string, unknown>)[key];
  }
  return JSON.stringify(clone);
}

function checkpointMaterial(record: {
  session_id: string;
  profile: string;
  state: JsrtSessionState;
  sequence: number;
  active_contract_refs: string[];
}): string {
  return JSON.stringify({
    session_id: record.session_id,
    profile: record.profile,
    state: record.state,
    sequence: record.sequence,
    active_contract_refs: [...record.active_contract_refs].sort(),
  });
}

function checkpointIntegrityMarker(record: {
  session_id: string;
  profile: string;
  state: JsrtSessionState;
  sequence: number;
  active_contract_refs: string[];
}): string {
  return createHash("sha256").update(checkpointMaterial(record)).digest("hex");
}

export function computeFrameChecksum(frame: JsrtFrame): string {
  const canonical = canonicalFrameForHash(frame);
  return createHash("sha256").update(canonical).digest("hex");
}

export function signFrameHmac(frame: JsrtFrame, secret: string): string {
  const canonical = canonicalFrameForHash(frame);
  const digest = createHmac("sha256", secret).update(canonical).digest("hex");
  return `hmac-sha256:${digest}`;
}

function isNamespacedCustomType(frameType: string): boolean {
  return /^[A-Za-z][A-Za-z0-9_]*\.[A-Za-z][A-Za-z0-9_]*\.[A-Za-z][A-Za-z0-9_]*$/.test(frameType);
}

function validateRequiredPayload(frame: JsrtFrame): JsrtIssue[] {
  const issues: JsrtIssue[] = [];
  if (!CORE_FRAME_TYPES.has(frame.frame_type as JsrtFrameType)) {
    return issues;
  }

  const required = FRAME_PAYLOAD_REQUIRED[frame.frame_type as JsrtFrameType] ?? [];
  for (const key of required) {
    if (!(key in frame.payload)) {
      issues.push({
        code: "SchemaError",
        message: `Frame '${frame.frame_id}' payload missing required key '${key}' for ${frame.frame_type}`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }
  }

  return issues;
}

function validatePayloadSemantics(frame: JsrtFrame): JsrtIssue[] {
  const issues: JsrtIssue[] = [];

  if (frame.frame_type === "ValidationResult" && typeof frame.payload.passed !== "boolean") {
    issues.push({
      code: "TypeMismatch",
      message: "ValidationResult.payload.passed must be boolean",
      frame_id: frame.frame_id,
      sequence: frame.sequence,
    });
  }

  if (frame.frame_type === "ArtifactReject" && !Array.isArray(frame.payload.reasons)) {
    issues.push({
      code: "TypeMismatch",
      message: "ArtifactReject.payload.reasons must be array",
      frame_id: frame.frame_id,
      sequence: frame.sequence,
    });
  }

  if (frame.frame_type === "CapabilityDeclare") {
    if (!Array.isArray(frame.payload.capabilities) || frame.payload.capabilities.some((x) => typeof x !== "string")) {
      issues.push({
        code: "TypeMismatch",
        message: "CapabilityDeclare.payload.capabilities must be string[]",
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }
  }

  if (frame.frame_type === "Error") {
    const severity = frame.payload.severity;
    const recoverability = frame.payload.recoverability;
    if (typeof severity !== "string" || !ERROR_SEVERITIES.has(severity)) {
      issues.push({
        code: "SchemaError",
        message: "Error.payload.severity must be one of Info|Warning|Error|Critical|Blocking",
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }
    if (typeof recoverability !== "string" || !ERROR_RECOVERABILITY.has(recoverability)) {
      issues.push({
        code: "SchemaError",
        message: "Error.payload.recoverability must be one of none|retryable|remediable|resumable|manual",
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }
    if (!isIsoDate(frame.payload.timestamp)) {
      issues.push({
        code: "TypeMismatch",
        message: "Error.payload.timestamp must be ISO date string",
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }
  }

  if (frame.supersedes && typeof frame.payload.replacement_reason !== "string") {
    issues.push({
      code: "SchemaError",
      message: "Frames using supersedes must include payload.replacement_reason",
      frame_id: frame.frame_id,
      sequence: frame.sequence,
    });
  }

  if (frame.retry_of && frame.supersedes) {
    issues.push({
      code: "SchemaError",
      message: "Frame cannot declare both retry_of and supersedes",
      frame_id: frame.frame_id,
      sequence: frame.sequence,
    });
  }

  return issues;
}

function pushTypeIssue(
  issues: JsrtIssue[],
  frame: Partial<JsrtFrame>,
  field: string,
  expected: string,
): void {
  issues.push({
    code: "TypeMismatch",
    message: `Field '${field}' must be ${expected}`,
    frame_id: frame.frame_id,
    sequence: frame.sequence as number | undefined,
  });
}

function validateArrayOfStrings(
  issues: JsrtIssue[],
  frame: Partial<JsrtFrame>,
  field: keyof JsrtFrame,
): void {
  const value = frame[field];
  if (value === undefined) return;
  if (!Array.isArray(value) || value.some((x) => typeof x !== "string")) {
    pushTypeIssue(issues, frame, String(field), "string[]");
  }
}

function validateFrameShape(input: unknown, profile: JsrtProfile, context?: JsrtContext): JsrtValidationResult {
  const issues: JsrtIssue[] = [];

  if (!isRecord(input)) {
    return {
      valid: false,
      issues: [{ code: "SchemaError", message: "Frame must be a JSON object" }],
    };
  }

  const frame = input as Partial<JsrtFrame>;

  for (const field of REQUIRED_FIELDS) {
    if (!(field in frame)) {
      issues.push({
        code: "MissingField",
        message: `Missing required field '${field}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence as number | undefined,
      });
    }
  }

  if (profile.unknown_field_policy === "reject") {
    for (const key of Object.keys(frame)) {
      if (!RESERVED_FIELDS.has(key)) {
        issues.push({
          code: "ProfileViolation",
          message: `Unknown top-level field '${key}' is not allowed in profile '${profile.name}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence as number | undefined,
        });
      }
    }
  }

  if (typeof frame.jsrt_version !== "string") {
    pushTypeIssue(issues, frame, "jsrt_version", "string");
  } else if (frame.jsrt_version !== "0.1") {
    issues.push({
      code: "SchemaError",
      message: `Unsupported jsrt_version '${frame.jsrt_version}', expected '0.1'`,
      frame_id: frame.frame_id,
      sequence: frame.sequence as number | undefined,
    });
  }

  if (typeof frame.frame_id !== "string") pushTypeIssue(issues, frame, "frame_id", "string");
  if (typeof frame.session_id !== "string") pushTypeIssue(issues, frame, "session_id", "string");
  if (typeof frame.frame_type !== "string") pushTypeIssue(issues, frame, "frame_type", "string");

  if (typeof frame.sequence !== "number" || !Number.isInteger(frame.sequence) || frame.sequence < 1) {
    pushTypeIssue(issues, frame, "sequence", "positive integer");
  }

  if (!isIsoDate(frame.timestamp)) pushTypeIssue(issues, frame, "timestamp", "ISO date string");

  if (typeof frame.state !== "string") {
    pushTypeIssue(issues, frame, "state", "string");
  } else if (!SESSION_STATES.has(frame.state as JsrtSessionState)) {
    issues.push({
      code: "SchemaError",
      message: `Unknown state '${frame.state}'`,
      frame_id: frame.frame_id,
      sequence: frame.sequence as number | undefined,
    });
  }

  if (!isRecord(frame.payload)) pushTypeIssue(issues, frame, "payload", "object");

  if (frame.profile !== undefined && typeof frame.profile !== "string") pushTypeIssue(issues, frame, "profile", "string");
  if (frame.source !== undefined && typeof frame.source !== "string") pushTypeIssue(issues, frame, "source", "string");
  if (frame.target !== undefined && typeof frame.target !== "string") pushTypeIssue(issues, frame, "target", "string");
  if (frame.stream_id !== undefined && typeof frame.stream_id !== "string") pushTypeIssue(issues, frame, "stream_id", "string");
  if (frame.correlation_id !== undefined && typeof frame.correlation_id !== "string") pushTypeIssue(issues, frame, "correlation_id", "string");
  if (frame.parent_frame_id !== undefined && typeof frame.parent_frame_id !== "string") pushTypeIssue(issues, frame, "parent_frame_id", "string");
  if (frame.checksum !== undefined && typeof frame.checksum !== "string") pushTypeIssue(issues, frame, "checksum", "string");
  if (frame.signature !== undefined && typeof frame.signature !== "string") pushTypeIssue(issues, frame, "signature", "string");
  if (frame.priority !== undefined && typeof frame.priority !== "number") pushTypeIssue(issues, frame, "priority", "number");
  if (frame.expires_at !== undefined && !isIsoDate(frame.expires_at)) pushTypeIssue(issues, frame, "expires_at", "ISO date string");
  if (frame.retry_of !== undefined && typeof frame.retry_of !== "string") pushTypeIssue(issues, frame, "retry_of", "string");
  if (frame.supersedes !== undefined && typeof frame.supersedes !== "string") pushTypeIssue(issues, frame, "supersedes", "string");

  validateArrayOfStrings(issues, frame, "tags");
  validateArrayOfStrings(issues, frame, "contract_refs");
  validateArrayOfStrings(issues, frame, "prompt_refs");
  validateArrayOfStrings(issues, frame, "capabilities");

  if (frame.annotations !== undefined && !isRecord(frame.annotations)) pushTypeIssue(issues, frame, "annotations", "object");
  if (frame.trace !== undefined && !isRecord(frame.trace)) pushTypeIssue(issues, frame, "trace", "object");
  if (frame.diagnostics !== undefined && !isRecord(frame.diagnostics)) pushTypeIssue(issues, frame, "diagnostics", "object");
  if (frame.checkpoint !== undefined && !isRecord(frame.checkpoint)) pushTypeIssue(issues, frame, "checkpoint", "object");

  const frameType = String(frame.frame_type ?? "");
  const known = CORE_FRAME_TYPES.has(frameType as JsrtFrameType);
  const customOk = profile.allow_custom_frame_types && isNamespacedCustomType(frameType);

  if (!known && !customOk) {
    issues.push({
      code: "UnknownFrameType",
      message: `Unknown frame_type '${frameType}'`,
      frame_id: frame.frame_id,
      sequence: frame.sequence as number | undefined,
    });
  }

  if (known && !profile.allowed_frame_types.has(frameType)) {
    issues.push({
      code: "ProfileViolation",
      message: `Frame type '${frameType}' is not enabled in profile '${profile.name}'`,
      frame_id: frame.frame_id,
      sequence: frame.sequence as number | undefined,
    });
  }

  if (isRecord(frame.payload)) {
    const payloadBytes = Buffer.byteLength(JSON.stringify(frame.payload), "utf8");
    if (payloadBytes > profile.max_payload_bytes) {
      issues.push({
        code: "PayloadOverflow",
        message: `Frame payload exceeds profile max size (${payloadBytes} > ${profile.max_payload_bytes})`,
        frame_id: frame.frame_id,
        sequence: frame.sequence as number | undefined,
      });
    }

    const frameBytes = Buffer.byteLength(JSON.stringify(frame), "utf8");
    if (frameBytes > profile.max_frame_bytes) {
      issues.push({
        code: "PayloadOverflow",
        message: `Frame envelope exceeds profile max size (${frameBytes} > ${profile.max_frame_bytes})`,
        frame_id: frame.frame_id,
        sequence: frame.sequence as number | undefined,
      });
    }
  }

  const now = context?.now ?? new Date();
  const expiresAt = typeof frame.expires_at === "string" ? frame.expires_at : undefined;
  const timestamp = typeof frame.timestamp === "string" ? frame.timestamp : undefined;
  if (expiresAt && isIsoDate(expiresAt)) {
    if (timestamp && isIsoDate(timestamp) && new Date(expiresAt).getTime() < new Date(timestamp).getTime()) {
      issues.push({
        code: "Timeout",
        message: `Frame '${String(frame.frame_id)}' expiration precedes timestamp`,
        frame_id: frame.frame_id,
        sequence: frame.sequence as number | undefined,
      });
    }
    if (new Date(expiresAt).getTime() < now.getTime()) {
      issues.push({
        code: "Timeout",
        message: `Frame '${String(frame.frame_id)}' is expired at evaluation time`,
        frame_id: frame.frame_id,
        sequence: frame.sequence as number | undefined,
      });
    }
  }

  const concrete = frame as JsrtFrame;
  if ((known || customOk) && isRecord(concrete.payload)) {
    issues.push(...validateRequiredPayload(concrete));
    if (known) {
      issues.push(...validatePayloadSemantics(concrete));
    }
  }

  if (profile.require_checksum || typeof frame.checksum === "string") {
    if (typeof frame.checksum !== "string") {
      issues.push({
        code: "ChecksumFailure",
        message: "Checksum is required by profile",
        frame_id: frame.frame_id,
        sequence: frame.sequence as number | undefined,
      });
    } else if (known || customOk) {
      const expected = computeFrameChecksum(concrete);
      if (frame.checksum !== expected) {
        issues.push({
          code: "ChecksumFailure",
          message: "Frame checksum verification failed",
          frame_id: frame.frame_id,
          sequence: frame.sequence as number | undefined,
        });
      }
    }
  }

  if (profile.require_signature || typeof frame.signature === "string") {
    if (typeof frame.signature !== "string") {
      issues.push({
        code: "SignatureFailure",
        message: "Signature is required by profile",
        frame_id: frame.frame_id,
        sequence: frame.sequence as number | undefined,
      });
    } else if (!context?.hmac_secret) {
      issues.push({
        code: "SignatureFailure",
        message: "Cannot verify signature without hmac_secret",
        frame_id: frame.frame_id,
        sequence: frame.sequence as number | undefined,
      });
    } else {
      const expected = signFrameHmac(concrete, context.hmac_secret);
      if (frame.signature !== expected) {
        issues.push({
          code: "SignatureFailure",
          message: "Frame signature verification failed",
          frame_id: frame.frame_id,
          sequence: frame.sequence as number | undefined,
        });
      }
    }
  }

  if (Array.isArray(frame.prompt_refs) && context?.prompt_registry) {
    const set = new Set(context.prompt_registry);
    for (const ref of frame.prompt_refs) {
      if (typeof ref !== "string" || !set.has(ref)) {
        issues.push({
          code: "ReferenceResolutionError",
          message: `Unresolved prompt_refs entry '${String(ref)}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence as number | undefined,
        });
      }
    }
  }

  if (Array.isArray(frame.contract_refs) && context?.contract_registry) {
    const set = new Set(context.contract_registry);
    for (const ref of frame.contract_refs) {
      if (typeof ref !== "string" || !set.has(ref)) {
        issues.push({
          code: "ReferenceResolutionError",
          message: `Unresolved contract_refs entry '${String(ref)}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence as number | undefined,
        });
      }
    }
  }

  return { valid: issues.length === 0, issues };
}

function validateSessionFlow(sessionId: string, frames: JsrtFrame[], profile: JsrtProfile): JsrtIssue[] {
  const issues: JsrtIssue[] = [];

  const sorted = [...frames].sort((a, b) => {
    if (a.sequence === b.sequence) return a.timestamp.localeCompare(b.timestamp);
    return a.sequence - b.sequence;
  });

  const seenIds = new Set<string>();
  const seenSeqs = new Map<number, JsrtFrame>();
  const blockingValidationTargets = new Set<string>();
  const pendingRemediationTargets = new Set<string>();

  let expected = 1;
  let state: JsrtSessionState | undefined;

  for (const frame of sorted) {
    if (seenIds.has(frame.frame_id)) {
      issues.push({
        code: "ReplayDetected",
        message: `Duplicate frame_id '${frame.frame_id}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }
    seenIds.add(frame.frame_id);

    const priorAtSequence = seenSeqs.get(frame.sequence);
    if (priorAtSequence) {
      if (profile.replay_policy !== "allow_retry") {
        issues.push({
          code: "ReplayDetected",
          message: `Duplicate sequence '${frame.sequence}' is not allowed by profile`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }
      if (!frame.retry_of && !frame.supersedes) {
        issues.push({
          code: "ReplayDetected",
          message: `Duplicate sequence '${frame.sequence}' must declare retry_of or supersedes`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }
    }
    seenSeqs.set(frame.sequence, frame);

    if (profile.sequence_mode === "strict") {
      if (frame.sequence !== expected) {
        issues.push({
          code: "InvalidSequence",
          message: `Session '${sessionId}' expected sequence ${expected} but got ${frame.sequence}`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }
      expected = frame.sequence + 1;
    } else {
      const gap = frame.sequence - expected;
      if (gap > profile.max_sequence_gap) {
        issues.push({
          code: "ProfileViolation",
          message: `Session '${sessionId}' sequence gap ${gap} exceeds profile max_sequence_gap ${profile.max_sequence_gap}`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }
      expected = Math.max(expected, frame.sequence + 1);
    }

    if (frame.sequence === 1 && frame.frame_type !== "SessionCreate") {
      issues.push({
        code: "UnknownSession",
        message: `Session '${sessionId}' must start with SessionCreate at sequence 1`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }

    if (!state) {
      state = frame.state;
    } else {
      if (TERMINAL_STATES.has(state) && frame.frame_type !== "Heartbeat" && !profile.allow_reopen_terminal) {
        issues.push({
          code: "IllegalTransition",
          message: `No active frames allowed after terminal state '${state}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }

      if (frame.frame_type === "Heartbeat" && frame.state !== state) {
        issues.push({
          code: "IllegalTransition",
          message: `Heartbeat must not mutate state (${state} -> ${frame.state})`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }

      const allowed = TRANSITIONS[state];
      if (!allowed.has(frame.state)) {
        issues.push({
          code: "IllegalTransition",
          message: `Illegal state transition ${state} -> ${frame.state}`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }

      state = frame.state;
    }

    const requiredState = FRAME_STATE_BY_TYPE[frame.frame_type as JsrtFrameType];
    if (requiredState && frame.state !== requiredState) {
      issues.push({
        code: "IllegalTransition",
        message: `Frame type '${frame.frame_type}' requires state '${requiredState}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }

    if (frame.parent_frame_id && !seenIds.has(frame.parent_frame_id)) {
      issues.push({
        code: "ReferenceResolutionError",
        message: `parent_frame_id '${frame.parent_frame_id}' must reference a prior frame in the session`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }

    if (frame.retry_of && !seenIds.has(frame.retry_of)) {
      issues.push({
        code: "ReplayDetected",
        message: `retry_of references unknown prior frame '${frame.retry_of}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }

    if (frame.supersedes && !seenIds.has(frame.supersedes)) {
      issues.push({
        code: "ReferenceResolutionError",
        message: `supersedes references unknown prior frame '${frame.supersedes}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
    }

    if (frame.frame_type === "ValidationRequest") {
      const target = typeof frame.payload.target_ref === "string" ? frame.payload.target_ref : undefined;
      if (target) blockingValidationTargets.add(target);
    }

    if (frame.frame_type === "ValidationResult") {
      const target = typeof frame.payload.target_ref === "string" ? frame.payload.target_ref : undefined;
      const passed = typeof frame.payload.passed === "boolean" ? frame.payload.passed : false;
      if (target) {
        blockingValidationTargets.delete(target);
        if (!passed) {
          pendingRemediationTargets.add(target);
        }
      }
    }

    if (frame.frame_type === "RemediationRequest") {
      const target = typeof frame.payload.target_ref === "string" ? frame.payload.target_ref : undefined;
      if (target) pendingRemediationTargets.add(target);
    }

    if (frame.frame_type === "RemediationResult") {
      const target = typeof frame.payload.target_ref === "string" ? frame.payload.target_ref : undefined;
      const status = typeof frame.payload.status === "string" ? frame.payload.status.toLowerCase() : "";
      if (target && (status === "resolved" || status === "success" || status === "completed")) {
        pendingRemediationTargets.delete(target);
      }
    }

    if (frame.frame_type === "Complete") {
      if (blockingValidationTargets.size > 0) {
        issues.push({
          code: "ContractViolation",
          message: "Complete frame is not allowed while blocking validations remain",
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }
      if (pendingRemediationTargets.size > 0) {
        issues.push({
          code: "ContractViolation",
          message: "Complete frame is not allowed while mandatory remediation remains",
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
      }
    }
  }

  return issues;
}

export function validateJsrtFrames(frames: unknown[], context: JsrtContext = {}): JsrtValidationResult {
  const profile = profileFor(context.profile);
  const issues: JsrtIssue[] = [];

  const validFramesBySession = new Map<string, JsrtFrame[]>();

  for (const raw of frames) {
    const shape = validateFrameShape(raw, profile, context);
    issues.push(...shape.issues);
    if (!shape.valid) continue;

    const frame = raw as JsrtFrame;
    const arr = validFramesBySession.get(frame.session_id) ?? [];
    arr.push(frame);
    validFramesBySession.set(frame.session_id, arr);
  }

  for (const [sessionId, sessionFrames] of validFramesBySession.entries()) {
    issues.push(...validateSessionFlow(sessionId, sessionFrames, profile));
  }

  return { valid: issues.length === 0, issues };
}

export function parseJsrtDocument(input: unknown, context: JsrtContext = {}): ParsedJsrtDocument {
  let framesRaw: unknown[];
  let profile = context.profile;
  let promptRegistry = context.prompt_registry;
  let contractRegistry = context.contract_registry;

  if (Array.isArray(input)) {
    framesRaw = input;
  } else if (isRecord(input) && Array.isArray(input.frames)) {
    framesRaw = input.frames as unknown[];
    if (typeof input.profile === "string") profile = input.profile;
    if (Array.isArray(input.prompt_registry)) {
      promptRegistry = input.prompt_registry.filter((x): x is string => typeof x === "string");
    }
    if (Array.isArray(input.contract_registry)) {
      contractRegistry = input.contract_registry.filter((x): x is string => typeof x === "string");
    }
  } else if (isRecord(input)) {
    framesRaw = [input];
  } else {
    const emptyDocument: JsrtDocument = { frames: [] };
    const issues: JsrtIssue[] = [{
      code: "SchemaError",
      message: "JSRT document must be frame object, array, or { frames: [...] }",
    }];
    return {
      document: emptyDocument,
      frames: emptyDocument.frames,
      issues,
    };
  }

  const effectiveContext: JsrtContext = {
    ...context,
    profile,
    prompt_registry: promptRegistry,
    contract_registry: contractRegistry,
  };

  const validation = validateJsrtFrames(framesRaw, effectiveContext);
  const profileResolved = profileFor(profile);
  const validFrames = framesRaw
    .filter((f) => validateFrameShape(f, profileResolved, effectiveContext).valid)
    .map((f) => f as JsrtFrame);

  const document: JsrtDocument = {
    profile,
    prompt_registry: promptRegistry,
    contract_registry: contractRegistry,
    frames: validFrames,
  };

  return {
    document,
    frames: validFrames,
    issues: validation.issues,
  };
}

export class JsrtSessionEngine {
  private readonly sessionId: string;
  private readonly profile: JsrtProfile;
  private readonly context: JsrtContext;

  private state: JsrtSessionState | undefined;
  private lastSequence = 0;
  private frameCount = 0;
  private readonly acceptedFrameIds: string[] = [];
  private readonly pendingFrameIds: string[] = [];
  private readonly seenFrameIds = new Set<string>();
  private readonly seenSequences = new Set<number>();
  private readonly capabilities = new Set<string>();
  private readonly activeContractRefs = new Set<string>();
  private readonly checkpoints = new Map<string, Omit<JsrtCheckpointRecord, "checkpoint_id">>();
  private readonly validationStatusByTarget = new Map<string, boolean>();
  private readonly pendingBySequence = new Map<number, JsrtFrame>();
  private readonly blockingValidationTargets = new Set<string>();
  private readonly pendingRemediationTargets = new Set<string>();

  constructor(sessionId: string, context: JsrtContext = {}) {
    this.sessionId = sessionId;
    this.context = context;
    this.profile = profileFor(context.profile);
  }

  private buildCheckpointIntegrityMarker(sequence: number, state: JsrtSessionState): string {
    return checkpointIntegrityMarker({
      session_id: this.sessionId,
      profile: this.profile.name,
      state,
      sequence,
      active_contract_refs: Array.from(this.activeContractRefs),
    });
  }

  private applyInternal(frame: JsrtFrame): JsrtApplyResult {
    const accepted: string[] = [];
    const pending: string[] = [];
    const rejected: string[] = [];
    const issues: JsrtIssue[] = [];

    const shape = validateFrameShape(frame, this.profile, this.context);
    if (!shape.valid) {
      rejected.push(frame.frame_id);
      issues.push(...shape.issues);
      return { accepted, pending, rejected, issues };
    }

    if (frame.session_id !== this.sessionId) {
      rejected.push(frame.frame_id);
      issues.push({
        code: "UnknownSession",
        message: `Frame session_id '${frame.session_id}' does not match engine session '${this.sessionId}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (this.seenFrameIds.has(frame.frame_id)) {
      rejected.push(frame.frame_id);
      issues.push({
        code: "ReplayDetected",
        message: `Duplicate frame_id '${frame.frame_id}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (this.seenSequences.has(frame.sequence)) {
      if (this.profile.replay_policy !== "allow_retry") {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ReplayDetected",
          message: `Duplicate sequence '${frame.sequence}' not allowed by profile`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }
      if (!frame.retry_of && !frame.supersedes) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ReplayDetected",
          message: `Duplicate sequence '${frame.sequence}' requires retry_of or supersedes`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }
    }

    if (this.frameCount === 0) {
      if (frame.sequence !== 1) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "InvalidSequence",
          message: "Session must start at sequence 1",
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }
      if (frame.frame_type !== "SessionCreate") {
        rejected.push(frame.frame_id);
        issues.push({
          code: "UnknownSession",
          message: "First frame must be SessionCreate",
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }
    }

    if (frame.sequence !== this.lastSequence + 1) {
      const gap = frame.sequence - (this.lastSequence + 1);
      if (gap > this.profile.max_sequence_gap) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ProfileViolation",
          message: `Sequence gap ${gap} exceeds profile max_sequence_gap ${this.profile.max_sequence_gap}`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }

      if (this.profile.sequence_mode === "hold_pending" && frame.sequence > this.lastSequence + 1) {
        this.pendingBySequence.set(frame.sequence, frame);
        this.pendingFrameIds.push(frame.frame_id);
        pending.push(frame.frame_id);
        return { accepted, pending, rejected, issues, snapshot: this.snapshot() };
      }

      rejected.push(frame.frame_id);
      issues.push({
        code: "InvalidSequence",
        message: `Expected sequence ${this.lastSequence + 1} but got ${frame.sequence}`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (this.state && TERMINAL_STATES.has(this.state) && frame.frame_type !== "Heartbeat" && !this.profile.allow_reopen_terminal) {
      rejected.push(frame.frame_id);
      issues.push({
        code: "IllegalTransition",
        message: `Session already in terminal state '${this.state}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (this.state && frame.frame_type === "Heartbeat" && frame.state !== this.state) {
      rejected.push(frame.frame_id);
      issues.push({
        code: "IllegalTransition",
        message: `Heartbeat must not mutate state (${this.state} -> ${frame.state})`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (this.state) {
      const allowed = TRANSITIONS[this.state];
      if (!allowed.has(frame.state)) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "IllegalTransition",
          message: `Illegal state transition ${this.state} -> ${frame.state}`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }
    }

    const requiredState = FRAME_STATE_BY_TYPE[frame.frame_type as JsrtFrameType];
    if (requiredState && frame.state !== requiredState) {
      rejected.push(frame.frame_id);
      issues.push({
        code: "IllegalTransition",
        message: `Frame type '${frame.frame_type}' requires state '${requiredState}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (frame.parent_frame_id && !this.seenFrameIds.has(frame.parent_frame_id)) {
      rejected.push(frame.frame_id);
      issues.push({
        code: "ReferenceResolutionError",
        message: `parent_frame_id '${frame.parent_frame_id}' must reference a prior frame`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (frame.retry_of && !this.seenFrameIds.has(frame.retry_of)) {
      rejected.push(frame.frame_id);
      issues.push({
        code: "ReplayDetected",
        message: `retry_of references unknown frame '${frame.retry_of}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (frame.supersedes && !this.seenFrameIds.has(frame.supersedes)) {
      rejected.push(frame.frame_id);
      issues.push({
        code: "ReferenceResolutionError",
        message: `supersedes references unknown frame '${frame.supersedes}'`,
        frame_id: frame.frame_id,
        sequence: frame.sequence,
      });
      return { accepted, pending, rejected, issues };
    }

    if (Array.isArray(frame.contract_refs)) {
      for (const c of frame.contract_refs) {
        if (typeof c === "string") this.activeContractRefs.add(c);
      }
    }

    if (frame.frame_type === "ValidationRequest") {
      const targetRef = typeof frame.payload.target_ref === "string" ? frame.payload.target_ref : undefined;
      if (targetRef) this.blockingValidationTargets.add(targetRef);
    }

    if (frame.frame_type === "ValidationResult") {
      const targetRef = typeof frame.payload.target_ref === "string" ? frame.payload.target_ref : undefined;
      const passed = typeof frame.payload.passed === "boolean" ? frame.payload.passed : false;
      if (targetRef) {
        this.validationStatusByTarget.set(targetRef, passed);
        this.blockingValidationTargets.delete(targetRef);
        if (!passed) this.pendingRemediationTargets.add(targetRef);
      }
    }

    if (frame.frame_type === "RemediationRequest") {
      const targetRef = typeof frame.payload.target_ref === "string" ? frame.payload.target_ref : undefined;
      if (targetRef) this.pendingRemediationTargets.add(targetRef);
    }

    if (frame.frame_type === "RemediationResult") {
      const targetRef = typeof frame.payload.target_ref === "string" ? frame.payload.target_ref : undefined;
      const status = typeof frame.payload.status === "string" ? frame.payload.status.toLowerCase() : "";
      if (targetRef && (status === "resolved" || status === "success" || status === "completed")) {
        this.pendingRemediationTargets.delete(targetRef);
      }
    }

    if (frame.frame_type === "ArtifactAccept") {
      const artifactId = typeof frame.payload.artifact_id === "string" ? frame.payload.artifact_id : undefined;
      if (artifactId && this.validationStatusByTarget.get(artifactId) !== true) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ContractViolation",
          message: `ArtifactAccept requires prior passing ValidationResult for '${artifactId}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }
    }

    if (frame.frame_type === "Complete") {
      if (this.blockingValidationTargets.size > 0) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ContractViolation",
          message: "Complete is blocked by unresolved ValidationRequest targets",
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }
      if (this.pendingRemediationTargets.size > 0) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ContractViolation",
          message: "Complete is blocked by unresolved remediation targets",
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }
    }

    if (frame.frame_type === "CapabilityDeclare" && Array.isArray(frame.payload.capabilities)) {
      for (const capability of frame.payload.capabilities) {
        if (typeof capability === "string") this.capabilities.add(capability);
      }
    }

    if (frame.frame_type === "CheckpointCreate") {
      const checkpointId = typeof frame.payload.checkpoint_id === "string"
        ? frame.payload.checkpoint_id
        : `cp-${frame.sequence}`;
      const integrityMarker = this.buildCheckpointIntegrityMarker(frame.sequence, frame.state);
      this.checkpoints.set(checkpointId, {
        frame_id: frame.frame_id,
        sequence: frame.sequence,
        state: frame.state,
        profile: this.profile.name,
        integrity_marker: integrityMarker,
        active_contract_refs: Array.from(this.activeContractRefs),
      });
    }

    if (frame.frame_type === "CheckpointRestore") {
      const checkpointId = typeof frame.payload.checkpoint_id === "string" ? frame.payload.checkpoint_id : undefined;
      const integrityHash = typeof frame.payload.integrity_hash === "string" ? frame.payload.integrity_hash : undefined;

      if (!checkpointId || !this.checkpoints.has(checkpointId)) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ReferenceResolutionError",
          message: `Unknown checkpoint '${String(checkpointId)}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }

      const cp = this.checkpoints.get(checkpointId)!;
      if (!integrityHash || integrityHash !== cp.integrity_marker) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ChecksumFailure",
          message: `CheckpointRestore integrity marker mismatch for checkpoint '${checkpointId}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }

      const restoreProfile = typeof frame.payload.profile === "string" ? frame.payload.profile : this.profile.name;
      if (restoreProfile !== this.profile.name || cp.profile !== this.profile.name) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "ProfileViolation",
          message: `CheckpointRestore profile mismatch for checkpoint '${checkpointId}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }

      if (frame.state !== cp.state) {
        rejected.push(frame.frame_id);
        issues.push({
          code: "IllegalTransition",
          message: `CheckpointRestore state '${frame.state}' must match checkpoint state '${cp.state}'`,
          frame_id: frame.frame_id,
          sequence: frame.sequence,
        });
        return { accepted, pending, rejected, issues };
      }

      this.activeContractRefs.clear();
      for (const c of cp.active_contract_refs) this.activeContractRefs.add(c);
    }

    this.lastSequence = frame.sequence;
    this.state = frame.state;
    this.frameCount += 1;
    this.seenFrameIds.add(frame.frame_id);
    this.seenSequences.add(frame.sequence);
    this.acceptedFrameIds.push(frame.frame_id);
    accepted.push(frame.frame_id);

    return {
      accepted,
      pending,
      rejected,
      issues,
      snapshot: this.snapshot(),
    };
  }

  apply(frame: JsrtFrame): JsrtApplyResult {
    const primary = this.applyInternal(frame);

    if (primary.rejected.length > 0 || primary.pending.length > 0) {
      return primary;
    }

    const accepted = [...primary.accepted];
    const pending: string[] = [...primary.pending];
    const rejected = [...primary.rejected];
    const issues = [...primary.issues];

    while (this.pendingBySequence.has(this.lastSequence + 1)) {
      const next = this.pendingBySequence.get(this.lastSequence + 1)!;
      this.pendingBySequence.delete(this.lastSequence + 1);
      const index = this.pendingFrameIds.indexOf(next.frame_id);
      if (index >= 0) this.pendingFrameIds.splice(index, 1);

      const follow = this.applyInternal(next);
      accepted.push(...follow.accepted);
      pending.push(...follow.pending);
      rejected.push(...follow.rejected);
      issues.push(...follow.issues);
      if (follow.rejected.length > 0 || follow.pending.length > 0) {
        break;
      }
    }

    return { accepted, pending, rejected, issues, snapshot: this.snapshot() };
  }

  snapshot(): JsrtSessionSnapshot {
    return {
      session_id: this.sessionId,
      profile: this.profile.name,
      state: this.state ?? "Created",
      last_sequence: this.lastSequence,
      frame_count: this.frameCount,
      accepted_frame_ids: [...this.acceptedFrameIds],
      pending_frame_ids: [...this.pendingFrameIds],
      capabilities: Array.from(this.capabilities),
      checkpoints: Array.from(this.checkpoints.entries()).map(([checkpoint_id, cp]) => ({
        checkpoint_id,
        ...cp,
      })),
      active_contract_refs: Array.from(this.activeContractRefs),
      blocking_validation_targets: Array.from(this.blockingValidationTargets),
      pending_remediation_targets: Array.from(this.pendingRemediationTargets),
    };
  }
}

export function applyJsrtFrames(frames: JsrtFrame[], context: JsrtContext = {}): JsrtApplyResult {
  if (frames.length === 0) {
    return {
      accepted: [],
      pending: [],
      rejected: [],
      issues: [{ code: "SchemaError", message: "No frames to apply" }],
    };
  }

  const engine = new JsrtSessionEngine(frames[0].session_id, context);

  const accepted: string[] = [];
  const pending: string[] = [];
  const rejected: string[] = [];
  const issues: JsrtIssue[] = [];
  let snapshot: JsrtSessionSnapshot | undefined;

  for (const frame of frames) {
    const result = engine.apply(frame);
    accepted.push(...result.accepted);
    pending.push(...result.pending);
    rejected.push(...result.rejected);
    issues.push(...result.issues);
    snapshot = result.snapshot ?? snapshot;
  }

  return { accepted, pending, rejected, issues, snapshot };
}
