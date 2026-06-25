# MSP Skiller Bundle Fixtures

This directory contains deterministic Skiller bundle fixtures used by MSP publisher and CLI conformance tests.

## `sample-rust-bundle/`

`sample-rust-bundle/` is a minimal producer-side Skiller export boundary with:

```text
sample-rust-bundle/
├── package.yaml
├── graph/
│   └── dependencies.yaml
├── skills/
│   └── refactor-module.yaml
└── sources/
    └── index.yaml
```

The fixture is intentionally small but complete enough for:

- `msp-publisher` import tests;
- `msp-cli publish import-skiller` unsigned and signed temp-registry conformance;
- generated MSP skill manifest validation;
- generated Markdown body hash verification;
- generated verification contract validation;
- generated skill pack manifest validation and member validation.

The expected normalized output identifiers are:

- skill: `skill.software_engineering.rust.refactor-module.v1`
- pack: `pack.sample-rust-bundle.v1`

Run the CLI publication conformance harness with:

```bash
cargo test -p msp-cli --test publish_import_conformance -- --nocapture
```

The test publishes into a unique temporary registry under the system temp directory and removes it on drop. It must not mutate `examples/registry` or any checked-in conformance fixture outputs.
