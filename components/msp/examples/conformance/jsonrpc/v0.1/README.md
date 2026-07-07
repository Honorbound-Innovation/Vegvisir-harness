# MSP JSON-RPC v0.1 Conformance Fixtures

This directory contains checked-in request/response fixtures for the MSP v0.1 JSON-RPC stdio method surface.

The fixtures are intentionally local-first and deterministic. They run against:

- registry root: `examples/registry`
- trust policy: `examples/policies/local-reference.trust-policy.json`
- execution report: `examples/reports/rust-refactor.report.json`

## Layout

```text
examples/conformance/jsonrpc/v0.1/
├── requests/
│   └── *.request.json
├── responses/
│   └── *.response.json
├── error-requests/
│   └── *.request.json
└── error-responses/
    └── *.response.json
```

Each success request fixture is a **single-line JSON-RPC 2.0 request** because the reference `msp-server` stdio transport is line-delimited JSON. Pretty-printed multi-line requests are not valid stdio frames for this server. Error request fixtures are also line-delimited frames; the parse-error fixture is intentionally malformed JSON on one line.

Each success or error response fixture is the exact single-line JSON-RPC response produced by:

```bash
cargo run -q -p msp-server -- --registry examples/registry \
  < examples/conformance/jsonrpc/v0.1/requests/<name>.request.json
```

## Covered Methods

The fixture set tracks `msp_core::core_methods()` exactly:

1. `msp.info`
2. `registry.search`
3. `skills.discover`
4. `skills.get_manifest`
5. `skills.load`
6. `skills.resolve_dependencies`
7. `skills.verify_result`
8. `skills.check_compatibility`
9. `packs.discover`
10. `packs.get_manifest`
11. `packs.load`
12. `packs.verify_trust`
13. `packs.evaluate_trust`
14. `packs.validate_members`
15. `packs.evaluate_dependencies`
16. `trust.verify`
17. `trust.evaluate`
18. `trust.evaluate_dependencies`

## Error Fixtures

The `error-requests/` and `error-responses/` fixtures cover representative JSON-RPC and MSP application error paths:

- parse error (`-32700`) for malformed JSON;
- invalid JSON-RPC version (`-32600`);
- unknown method (`-32601`);
- missing required id (`-32000`);
- missing required trust policy (`-32000`);
- invalid enum parameter deserialization (`-32000`);
- missing skill lookup (`-32000`).

The v0.1 reference server currently maps application/dispatch failures to JSON-RPC `-32000` with a human-readable message.

## Validation

The integration test at `crates/msp-server/tests/jsonrpc_conformance.rs` enforces that:

- every fixture method matches the advertised `core_methods()` order;
- every checked-in request fixture is valid JSON-RPC 2.0 with the expected method name;
- every checked-in success response fixture is a successful JSON-RPC response;
- every success response `result` validates against its canonical MSP schema where one exists;
- every checked-in error response fixture has the expected JSON-RPC error code and id behavior;
- running the compiled `msp-server` binary against all success and error request fixtures reproduces the response fixtures byte-for-byte.

Run it directly with:

```bash
cargo test -p msp-server --test jsonrpc_conformance -- --nocapture
```

## Regenerating Fixtures

If an intentional protocol/result change is made, regenerate response fixtures from the workspace root:

```bash
for f in examples/conformance/jsonrpc/v0.1/requests/*.request.json; do
  base=$(basename "$f" .request.json)
  cargo run -q -p msp-server -- --registry examples/registry \
    < "$f" \
    > "examples/conformance/jsonrpc/v0.1/responses/$base.response.json"
done

for f in examples/conformance/jsonrpc/v0.1/error-requests/*.request.json; do
  base=$(basename "$f" .request.json)
  cargo run -q -p msp-server -- --registry examples/registry \
    < "$f" \
    > "examples/conformance/jsonrpc/v0.1/error-responses/$base.response.json"
done
```

Then run:

```bash
cargo test -p msp-server --test jsonrpc_conformance -- --nocapture
cargo test --workspace
```

Do not regenerate fixtures to hide drift. Regeneration should accompany an intentional protocol, schema, registry fixture, or serializer change.
