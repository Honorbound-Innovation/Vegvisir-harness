import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";

import {
  createAuthSessionProfile,
  readAuthSessionProfile,
  resolveAuthSession,
  validateAuthSessionProfile
} from "../dist/index.js";

test("createAuthSessionProfile writes safe storage-state metadata", async () => {
  const dir = await mkdtemp(join(tmpdir(), "solarium-auth-"));
  const output = join(dir, "staging.auth-session.json");
  try {
    const profile = await createAuthSessionProfile({
      output,
      name: "staging-admin",
      storageState: ".solarium/auth/staging-admin.state.json",
      description: "Staging admin browser state",
      secretRefs: ["hbse://project/staging-admin"],
      metadata: { environment: "staging" }
    });

    assert.equal(profile.schemaVersion, "solarium.auth-session.v1");
    assert.equal(profile.name, "staging-admin");
    assert.deepEqual(profile.secretRefs, ["hbse://project/staging-admin"]);

    const read = await readAuthSessionProfile(output);
    assert.equal(read.storageState, ".solarium/auth/staging-admin.state.json");
    assert.equal(read.metadata.environment, "staging");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("validateAuthSessionProfile rejects malformed profiles", () => {
  assert.throws(() => validateAuthSessionProfile({}), /schemaVersion/);
  assert.throws(() => validateAuthSessionProfile({ schemaVersion: "solarium.auth-session.v1", name: "x" }), /storageState/);
  assert.throws(() => validateAuthSessionProfile({
    schemaVersion: "solarium.auth-session.v1",
    name: "x",
    storageState: "state.json",
    createdAt: "now",
    updatedAt: "now",
    secretRefs: [""]
  }), /secretRefs/);
});

test("resolveAuthSession prefers explicit paths but can load profile defaults", async () => {
  const dir = await mkdtemp(join(tmpdir(), "solarium-auth-resolve-"));
  const profilePath = join(dir, "default.auth-session.json");
  try {
    await createAuthSessionProfile({ output: profilePath, name: "default", storageState: "profile-state.json" });

    const fromProfile = await resolveAuthSession({ profilePath });
    assert.equal(fromProfile.storageState, "profile-state.json");
    assert.equal(fromProfile.saveStorageState, "profile-state.json");

    const override = await resolveAuthSession({ profilePath, storageState: "override-state.json", saveStorageState: "save.json" });
    assert.equal(override.storageState, "override-state.json");
    assert.equal(override.saveStorageState, "save.json");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
