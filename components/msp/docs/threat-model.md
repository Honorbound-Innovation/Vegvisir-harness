# MSP v0.1 Threat Model

MSP distributes skills: structured procedural instructions that can influence how an AI runtime reasons and acts. A malicious skill may be dangerous even when it never directly calls a tool.

## Security Boundary

MSP artifacts may guide task execution, but they must not override:

- host policy;
- user policy;
- tool permissions;
- memory policy;
- identity policy;
- sandbox restrictions;
- approval requirements.

A skill can advise. It cannot grant itself authority.

## Primary Assets

The v0.1 security model protects:

- skill body integrity;
- manifest integrity;
- pack membership integrity;
- dependency graph transparency;
- issuer and provenance metadata;
- runtime trust-policy decisions;
- local registry path boundaries;
- reproducibility of published versions.

## Threats

### Malicious skill body

A skill body can attempt to manipulate the runtime's behavior, bypass verification, request unsafe actions, or weaken policy. Hosts must treat skill content as untrusted model-facing input unless trust policy allows it.

Mitigations:

- body hashing;
- optional detached Ed25519 signatures;
- issuer policy checks;
- risk and review gates;
- host-policy supremacy;
- forbidden behavior metadata;
- explicit verification contracts.

### Tampered artifact bytes

An attacker or broken publisher may alter a body after manifest publication.

Mitigations:

- `trust.hash` verification;
- detached signature verification over body bytes;
- load-time hash/signature checks;
- immutable-by-default publication.

### Tampered pack metadata

A pack can be altered to add, remove, or redirect members and dependencies.

Mitigations:

- canonical pack proof bytes;
- pack trust hash/signature checks;
- pack member URI safety;
- member ID/version/index validation;
- duplicate member rejection.

### Path traversal and local file exposure

A malicious manifest or pack may attempt to reference files outside the registry root.

Mitigations:

- reject absolute paths;
- reject `file:` URIs;
- reject external schemes;
- reject `..` escapes;
- interpret artifact URIs as registry-root-relative only.

### Hidden or unsafe dependencies

A skill may depend on untrusted skills or packs, or use a dependency graph to bypass policy.

Mitigations:

- explicit dependency declarations;
- missing-dependency reporting;
- cycle detection;
- deterministic version strategy checks;
- dependency trust evaluation;
- transitive-dependency policy gates;
- per-edge issuer/hash/signing constraints.

### Mutable published versions

Changing same-ID/same-version artifacts undermines reproducibility and trust.

Mitigations:

- duplicate publication rejection;
- idempotent-only `--force` by default;
- explicit `--allow-mutable-version` local escape hatch;
- recommended new-version release path;
- deprecation/replacement metadata.

### Over-trusting unsigned local artifacts

Unsigned local development artifacts are useful but should not be mistaken for production trust.

Mitigations:

- trust policies can require signatures;
- trusted issuer public-key bindings;
- signed publication support;
- review-required policy gates.

## Trust Policy Capabilities

The v0.1 evaluator supports deterministic local policy checks over:

- issuer allow/deny rules;
- issuer public-key references;
- risk thresholds;
- signature requirements;
- review status;
- forbidden-behavior warnings;
- skill and pack permissions;
- dependency trust requirements;
- dependency issuer/hash/signing constraints.

## Non-Goals in v0.1

The v0.1 reference implementation does not yet provide:

- remote registry authentication;
- network transport security;
- key transparency;
- global revocation;
- federated trust-anchor discovery;
- malware sandboxing;
- runtime tool-permission enforcement.

Those responsibilities remain with the host runtime or future MSP phases.

## Host Runtime Obligations

A secure runtime consuming MSP skills should:

1. load skills only through a policy-aware path;
2. never let skill content override higher-priority instructions or local policy;
3. avoid auto-loading high-risk skills without explicit review;
4. require signatures for non-local or production registries;
5. display or log skill provenance and trust decisions;
6. verify execution reports against declared verification contracts;
7. keep tool permissions independent from skill manifests.
