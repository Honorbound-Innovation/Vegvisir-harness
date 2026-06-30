# Vegvisir MSP Client

`msp-client` is a native Vegvisir component for consuming Model Skill Protocol
(MSP) local registries directly, without using MCP as a transport or tool
adapter.

The component wraps the vendored MSP reference registry crates under
`components/msp` and provides:

- a Rust library API (`MspClient`) suitable for future in-process Vegvisir tool
  registration;
- a JSON CLI (`msp-client`) for component smoke tests, packaging, and manual
  operation;
- bounded search/load defaults intended for model-facing observations;
- Skiller bundle import into canonical MSP registry artifacts using the vendored MSP publisher.

## Build

From the Vegvisir monorepo root:

```bash
cargo build -p msp-client
```

## CLI examples

Use the vendored reference registry:

```bash
cargo run -p msp-client -- --registry components/msp/examples/registry info
cargo run -p msp-client -- --registry components/msp/examples/registry summary
cargo run -p msp-client -- --registry components/msp/examples/registry search --task "refactor rust module" --tool read_file --tool write_file
# Import a current Skiller bundle into a local MSP registry. Writes skills/<id>/ and packs/<id>/ artifacts.
cargo run -p msp-client -- --registry tmp/msp-registry import-skiller components/msp/examples/skiller-bundles/sample-rust-bundle --issuer local.dev
cargo run -p msp-client -- --registry components/msp/examples/registry load skill.rust.refactor.module.v1 --mode card
cargo run -p msp-client -- --registry components/msp/examples/registry check-compatibility skill.rust.refactor.module.v1 --tool read_file --tool write_file --runtime-capability workspace_read --runtime-capability workspace_write
```

All command output is JSON.

## Library surface

The core type is `MspClient`:

```rust
use msp_client::{LoadMode, MspClient, SearchRequest};

let client = MspClient::open("components/msp/examples/registry")?;
let hits = client.search(SearchRequest {
    task: Some("refactor rust module".to_string()),
    limit: Some(5),
    ..SearchRequest::default()
});
let loaded = client.load_skill("skill.rust.refactor.module.v1", LoadMode::Card)?;
# Ok::<(), anyhow::Error>(())
```

Supported operations:

- `info`
- `summary`
- `search`
- `import_skiller_bundle`
- `load_skill`
- `get_manifest`
- `verify_trust`
- `evaluate_trust`
- `check_compatibility`
- `resolve_dependencies`
- `discover_packs`
- `get_pack_manifest`

## Integration intent

Vegvisir exposes this through native MSP tools including `msp_client_import_skiller` (risky/write), `msp_client_search`, `msp_client_load`, and trust/compatibility checks.

This crate is deliberately separate from MCP. It is the in-process client layer
Vegvisir can later expose through first-class tools/commands while still using
MSP's local registry and schema/trust model.
