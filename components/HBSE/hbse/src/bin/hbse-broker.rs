use std::path::PathBuf;

use clap::Parser;
use hbse::broker_daemon::HttpGatewayConfig;

#[derive(Debug, Parser)]
#[command(name = "hbse-broker")]
#[command(about = "Hardware Bound Secrets Enclave broker daemon")]
struct Cli {
    #[arg(long)]
    vault: PathBuf,
    #[arg(long)]
    socket: PathBuf,
    #[arg(long, default_value_t = 0.0)]
    idle_timeout_seconds: f64,
    #[arg(long)]
    http_listen: Option<String>,
    #[arg(long)]
    http_upstream_base_url: Option<String>,
    #[arg(long)]
    http_secret_ref: Option<String>,
    #[arg(long, default_value = "hbse.http-gateway")]
    http_consumer: String,
    #[arg(long, default_value = "model.chat")]
    http_purpose: String,
    #[arg(long, default_value = "model.discovery")]
    http_model_discovery_purpose: String,
    #[arg(long, default_value = "Authorization")]
    http_credential_header: String,
    #[arg(long, default_value = "Bearer ")]
    http_credential_prefix: String,
    #[arg(long, default_value_t = 0.0)]
    http_timeout_seconds: f64,
    #[arg(long, default_value_t = 10 * 1024 * 1024)]
    http_max_response_bytes: u64,
}

fn main() {
    let cli = Cli::parse();
    let http_gateway = match cli.http_listen {
        Some(listen) => {
            let Some(upstream_base_url) = cli.http_upstream_base_url else {
                eprintln!("Error: --http-upstream-base-url is required with --http-listen");
                std::process::exit(2);
            };
            let Some(secret_ref) = cli.http_secret_ref else {
                eprintln!("Error: --http-secret-ref is required with --http-listen");
                std::process::exit(2);
            };
            Some(HttpGatewayConfig {
                listen,
                upstream_base_url,
                secret_ref,
                consumer: cli.http_consumer,
                purpose: cli.http_purpose,
                model_discovery_purpose: cli.http_model_discovery_purpose,
                credential_header: cli.http_credential_header,
                credential_prefix: cli.http_credential_prefix,
                timeout_seconds: cli.http_timeout_seconds,
                max_response_bytes: cli.http_max_response_bytes,
            })
        }
        None => None,
    };
    if let Err(err) = hbse::broker_daemon::serve_with_http_gateway(
        cli.vault,
        cli.socket,
        cli.idle_timeout_seconds,
        http_gateway,
    ) {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}
