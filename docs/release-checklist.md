# Release Checklist

Run before release:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace --no-fail-fast
cargo run -p vegvisir-rust -- eval all
cargo run -p vegvisir-rust -- verify all
cargo run -p vegvisir-rust -- setup --check
```
