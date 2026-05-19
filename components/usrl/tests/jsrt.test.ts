import test from "node:test";
import assert from "node:assert/strict";

import {
  applyJsrtFrames,
  computeFrameChecksum,
  parseJsrtDocument,
  signFrameHmac,
  type JsrtFrame,
} from "../src/jsrt.js";

function mkFrame(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    jsrt_version: "0.1",
    frame_id: "f1",
    session_id: "s1",
    sequence: 1,
    frame_type: "SessionCreate",
    timestamp: "2026-04-10T00:00:00Z",
    state: "Created",
    payload: { profile: "StrictDeterministic", purpose: "test-session" },
    ...overrides,
  };
}

test("validates and applies basic jsrt session stream", () => {
  const frames = [
    mkFrame(),
    mkFrame({ frame_id: "f2", sequence: 2, frame_type: "SessionInit", state: "Initialized" }),
    mkFrame({ frame_id: "f3", sequence: 3, frame_type: "StateTransition", state: "Ready" }),
    mkFrame({ frame_id: "f4", sequence: 4, frame_type: "StateTransition", state: "Active" }),
    mkFrame({ frame_id: "f5", sequence: 5, frame_type: "Complete", state: "Completed" }),
  ];

  const parsed = parseJsrtDocument(frames);
  assert.equal(parsed.issues.length, 0);

  const applied = applyJsrtFrames(parsed.document.frames);
  assert.equal(applied.issues.length, 0);
  assert.equal(applied.accepted.length, 5);
  assert.equal(applied.snapshot?.state, "Completed");
});

test("detects invalid sequence and illegal transition", () => {
  const frames = [
    mkFrame(),
    mkFrame({ frame_id: "f2", sequence: 3, frame_type: "SessionInit", state: "Initialized" }),
    mkFrame({ frame_id: "f3", sequence: 4, frame_type: "StateTransition", state: "Completed" }),
    mkFrame({ frame_id: "f4", sequence: 5, frame_type: "PromptDispatch", state: "Active" }),
  ];

  const parsed = parseJsrtDocument(frames);
  assert.ok(parsed.issues.some((i) => i.code === "InvalidSequence"));
  assert.ok(parsed.issues.some((i) => i.code === "IllegalTransition"));
});

test("supports wrapped jsrt document with frames key", () => {
  const doc = {
    frames: [
      mkFrame(),
      mkFrame({ frame_id: "f2", sequence: 2, frame_type: "SessionInit", state: "Initialized" }),
    ],
  };

  const parsed = parseJsrtDocument(doc);
  assert.equal(parsed.document.frames.length, 2);
  assert.equal(parsed.issues.length, 0);
});

test("rejects unsupported jsrt version", () => {
  const parsed = parseJsrtDocument([mkFrame({ jsrt_version: "9.9" })]);
  assert.ok(parsed.issues.some((i) => i.code === "SchemaError" && i.message.includes("Unsupported jsrt_version")));
});

test("enforces heartbeat non-mutation of state", () => {
  const frames = [
    mkFrame(),
    mkFrame({ frame_id: "f2", sequence: 2, frame_type: "SessionInit", state: "Initialized" }),
    mkFrame({ frame_id: "f3", sequence: 3, frame_type: "Heartbeat", state: "Active", payload: {} }),
  ];

  const parsed = parseJsrtDocument(frames);
  assert.ok(parsed.issues.some((i) => i.code === "IllegalTransition" && i.message.includes("Heartbeat")));
});

test("blocks completion when remediation remains unresolved", () => {
  const frames = [
    mkFrame(),
    mkFrame({ frame_id: "f2", sequence: 2, frame_type: "SessionInit", state: "Initialized", payload: {} }),
    mkFrame({ frame_id: "f3", sequence: 3, frame_type: "StateTransition", state: "Ready", payload: {} }),
    mkFrame({ frame_id: "f4", sequence: 4, frame_type: "StateTransition", state: "Active", payload: {} }),
    mkFrame({ frame_id: "f5", sequence: 5, frame_type: "ValidationRequest", state: "AwaitingValidation", payload: { target_ref: "A1" } }),
    mkFrame({ frame_id: "f6", sequence: 6, frame_type: "ValidationResult", state: "AwaitingRemediation", payload: { target_ref: "A1", passed: false } }),
    mkFrame({ frame_id: "f7", sequence: 7, frame_type: "StateTransition", state: "Active", payload: {} }),
    mkFrame({ frame_id: "f8", sequence: 8, frame_type: "Complete", state: "Completed", payload: {} }),
  ];

  const applied = applyJsrtFrames(parseJsrtDocument(frames).document.frames);
  assert.ok(applied.issues.some((i) => i.code === "ContractViolation" && i.message.includes("remediation")));
  assert.ok(applied.rejected.includes("f8"));
});

test("development profile allows hold-pending and custom namespaced frame types", () => {
  const frames = [
    mkFrame({ profile: "Development" }),
    mkFrame({
      frame_id: "f3",
      sequence: 3,
      frame_type: "Vendor.Router.CustomFrame",
      state: "Ready",
      payload: {},
      profile: "Development",
    }),
    mkFrame({ frame_id: "f2", sequence: 2, frame_type: "SessionInit", state: "Initialized", payload: {}, profile: "Development" }),
  ];

  const parsed = parseJsrtDocument({ profile: "Development", frames });
  assert.equal(parsed.issues.length, 0);
  const applied = applyJsrtFrames(parsed.document.frames, { profile: "Development" });
  assert.equal(applied.rejected.length, 0);
  assert.equal(applied.snapshot?.last_sequence, 3);
});

test("high-security profile validates checksum and signature", () => {
  const secret = "test-secret";
  const f1 = mkFrame({ profile: "HighSecurity", payload: { purpose: "x" } }) as JsrtFrame;
  f1.checksum = computeFrameChecksum(f1);
  f1.signature = signFrameHmac(f1, secret);

  const f2 = mkFrame({
    frame_id: "f2",
    sequence: 2,
    frame_type: "SessionInit",
    state: "Initialized",
    payload: {},
    profile: "HighSecurity",
  }) as JsrtFrame;
  f2.checksum = computeFrameChecksum(f2);
  f2.signature = signFrameHmac(f2, secret);

  const parsed = parseJsrtDocument({ profile: "HighSecurity", frames: [f1, f2] }, { hmac_secret: secret });
  assert.equal(parsed.issues.length, 0);
});
