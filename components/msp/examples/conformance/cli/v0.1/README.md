# MSP CLI v0.1 Conformance Fixtures

This directory contains checked-in command/stdout/stderr/status fixtures for the MSP v0.1 reference CLI.

The fixtures are deterministic local-first examples. They run from the workspace root against:

- CLI binary: `msp-cli`
- registry root: `examples/registry`
- trust policy: `examples/policies/local-reference.trust-policy.json`
- execution report: `examples/reports/rust-refactor.report.json`

## Layout

```text
examples/conformance/cli/v0.1/
├── commands/
│   └── *.args.json
├── stdout/
│   └── *.stdout.txt
├── stderr/
│   └── *.stderr.txt
├── status/
│   └── *.status.json
├── error-commands/
│   └── *.args.json
├── error-stdout/
│   └── *.stdout.txt
├── error-stderr/
│   └── *.stderr.txt
└── error-status/
    └── *.status.json
```

Each command fixture stores only the arguments passed after the binary name as a JSON string array. For example:

```json
[
  "--registry",
  "examples/registry",
  "skills",
  "load",
  "skill.rust.refactor.module.v1"
]
```

The corresponding stdout, stderr, and status fixtures are the exact output from running:

```bash
cargo run -q -p msp-cli -- $(jq -r '.[]' examples/conformance/cli/v0.1/commands/<name>.args.json)
```

Most success stdout fixtures are pretty-printed JSON because the reference CLI prints JSON output. `hash` is the exception: it prints one `sha256:<hex>` line. Error fixtures intentionally have empty stdout, non-empty stderr, and non-zero status metadata.

## Covered Success Commands

The success fixture set covers the stable local v0.1 CLI surface:

1. `info`
2. `registry index`
3. `registry search`
4. `skills discover`
5. `skills manifest`
6. `skills load`
7. `skills resolve-dependencies`
8. `skills verify-result`
9. `skills check-compatibility`
10. `packs discover`
11. `packs manifest`
12. `packs load`
13. `packs verify-trust`
14. `packs evaluate-trust`
15. `packs validate-members`
16. `packs evaluate-dependencies`
17. `trust verify`
18. `trust verify-body`
19. `trust evaluate`
20. `trust evaluate-dependencies`
21. `hash`

`publish import-skiller` is intentionally not included in the static checked-in stdout/stderr fixture set because it writes registry artifacts and reports run-specific temp paths. It is covered separately by `crates/msp-cli/tests/publish_import_conformance.rs`, which publishes `examples/skiller-bundles/sample-rust-bundle/` into a unique temporary registry and validates unsigned and signed generated reports against `publication-report.schema.json` plus generated artifacts without mutating checked-in fixtures.

## Covered Error Commands

The error fixture set locks representative command-level failures:

1. missing skill manifest lookup
2. missing pack manifest lookup
3. invalid `--max-risk` value
4. missing trust-policy file
5. missing hash input file
6. invalid `--tool-version` parser input
7. missing execution-report file

Most application errors currently exit with code `1`. Clap argument parser errors exit with code `2`.

## Validation

The integration test at `crates/msp-cli/tests/cli_conformance.rs` enforces that:

- every success and error command fixture is well-formed JSON args;
- every success status fixture records exit code `0`;
- every error status fixture records a non-zero exit code;
- every success stderr fixture is empty;
- every error stdout fixture is empty;
- every error stderr fixture contains an expected actionable diagnostic fragment;
- every JSON stdout fixture parses and validates against its canonical MSP schema where one exists;
- alias pairs stay equivalent:
  - `registry search` == `skills discover`
  - `packs manifest` == `packs load`
  - `trust verify` == `trust verify-body` for the current reference skill;
- running the compiled `msp-cli` binary from the workspace root reproduces success and error stdout, stderr, and status fixtures byte-for-byte.

Run it directly with:

```bash
cargo test -p msp-cli --test cli_conformance -- --nocapture
```

## Regenerating Fixtures

If an intentional CLI output, schema, serializer, registry fixture, error diagnostic, or command-contract change is made, regenerate fixtures from the workspace root with a controlled script or command loop, then run:

```bash
cargo test -p msp-cli --test cli_conformance -- --nocapture
cargo test --workspace
```

Do not regenerate fixtures to hide drift. Fixture changes should accompany an intentional CLI contract change.
