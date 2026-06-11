fn main() -> anyhow::Result<()> {
    vegvisir_rust::agent_admin::run_agent_admin_cli()
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_admin_bin_exposes_library_entrypoint() {
        let entrypoint: fn() -> anyhow::Result<()> =
            vegvisir_rust::agent_admin::run_agent_admin_cli;
        let _ = entrypoint;
    }
}
