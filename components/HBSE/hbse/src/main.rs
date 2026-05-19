use std::env;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};

use clap::{Parser, Subcommand};
use hbse::backup::{create_backup, restore_backup};
use hbse::broker_daemon;
use hbse::dotenv::{parse_dotenv, scan_dotenv, split_dotenv_values};
use hbse::policy::{AccessPolicy, AccessRequest, DeliveryMode};
use hbse::provider::PASSPHRASE_PROVIDER_ID;
use hbse::provider_catalog::local_provider_catalog;
use hbse::provider_system::{SystemFingerprintProvider, SYSTEM_FINGERPRINT_PROVIDER_ID};
use hbse::provider_tpm2::{LinuxTpm2ToolsProvider, TPM2_PROVIDER_ID};
use hbse::provider_tpm2_esapi::{LinuxTpm2EsapiProvider, TPM2_ESAPI_PROVIDER_ID};
use hbse::provider_yubikey::YubikeyPivProvider;
use hbse::records::SecretType;
use hbse::recovery::{
    generate_mnemonic_phrase, normalize_mnemonic_phrase, RecoveryManager, RecoveryPackage,
};
use hbse::release::{
    generate_release_evidence, generate_signing_keypair, sign_release_artifacts,
    verify_release_evidence,
};
use hbse::store::SQLiteVaultStore;
use hbse::systemd::{install_broker_service, BrokerServiceInstallOptions};
use hbse::vault::{delivery_mode_string, rotation_state_string, status_string, LocalVault};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Parser)]
#[command(name = "hbse")]
#[command(about = "Hardware Bound Secrets Enclave")]
struct Cli {
    #[arg(long, default_value_os_t = default_store_path())]
    vault: PathBuf,
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
    Vault {
        #[command(subcommand)]
        command: VaultCommand,
    },
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Ticket {
        #[command(subcommand)]
        command: TicketCommand,
    },
    Rotation {
        #[command(subcommand)]
        command: RotationCommand,
    },
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    ModelProvider {
        #[command(subcommand)]
        command: ModelProviderCommand,
    },
    Mfa {
        #[command(subcommand)]
        command: MfaCommand,
    },
    Broker {
        #[command(subcommand)]
        command: BrokerCommand,
    },
    Dotenv {
        #[command(subcommand)]
        command: DotenvCommand,
    },
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
    Run {
        #[arg(long, default_value = "cli")]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long = "secret-env")]
        secret_env: Vec<String>,
        #[arg(long = "secret-file-env")]
        secret_file_env: Vec<String>,
        #[arg(long = "secret-fd-env")]
        secret_fd_env: Vec<String>,
        #[arg(long = "secret-stdin")]
        secret_stdin: Option<String>,
        #[arg(long = "env")]
        env: Vec<String>,
        #[arg(long)]
        mfa_code: Option<String>,
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },
    Resolve {
        secret_ref: String,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        broker: bool,
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
        #[arg(long, default_value = "cli")]
        consumer: String,
        #[arg(long, default_value = "terminal")]
        purpose: String,
        #[arg(long, default_value = "terminal_print")]
        delivery_mode: String,
        #[arg(long)]
        allow_plaintext: bool,
        #[arg(long)]
        mfa_code: Option<String>,
    },
    Doctor,
    Setup {
        #[arg(long, default_value = "/dev/tpmrm0")]
        tpm_device: String,
    },
    Lockdown {
        #[arg(long, default_value = "local lockdown")]
        reason: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Readiness {
        #[command(subcommand)]
        command: ReadinessCommand,
    },
}

#[derive(Debug, Subcommand)]
enum VaultCommand {
    Init {
        #[arg(long, default_value = "default")]
        namespace: String,
        #[arg(long, default_value = "passphrase")]
        provider: String,
        #[arg(long, default_value = "/dev/tpmrm0")]
        tpm_device: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Status,
    Backup {
        destination: PathBuf,
    },
    Restore {
        source: PathBuf,
    },
    RecoveryCreate {
        destination: PathBuf,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        recovery_secret: Option<String>,
        #[arg(long)]
        mnemonic: bool,
    },
    RecoveryInspect {
        package: PathBuf,
        #[arg(long)]
        recovery_secret: Option<String>,
        #[arg(long)]
        recovery_mnemonic: Option<String>,
    },
    Recover {
        package: PathBuf,
        #[arg(long)]
        recovery_secret: Option<String>,
        #[arg(long)]
        recovery_mnemonic: Option<String>,
        #[arg(long)]
        new_provider: String,
        #[arg(long)]
        new_passphrase: Option<String>,
        #[arg(long, default_value = "/dev/tpmrm0")]
        tpm_device: String,
    },
}

#[derive(Debug, Subcommand)]
enum SecretCommand {
    Put {
        secret_ref: String,
        #[arg(long)]
        value: Option<String>,
        #[arg(long)]
        stdin: bool,
        #[arg(long, default_value = "generic")]
        secret_type: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Get {
        secret_ref: String,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        allow_plaintext: bool,
        #[arg(long)]
        mfa_code: Option<String>,
    },
    Inspect {
        secret_ref: String,
    },
    Disable {
        secret_ref: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Destroy {
        secret_ref: String,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        reason: String,
    },
    List,
}

#[derive(Debug, Subcommand)]
enum AuditCommand {
    List {
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Export {
        destination: PathBuf,
        #[arg(long)]
        event_type: Option<String>,
    },
    Verify {
        #[arg(long)]
        passphrase: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum PolicyCommand {
    Put {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        passphrase: Option<String>,
    },
    List,
    Export {
        destination: Option<PathBuf>,
    },
    Hash {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        stdin: bool,
    },
    Test {
        #[arg(long)]
        secret_ref: String,
        #[arg(long)]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "brokered_http")]
        delivery_mode: String,
        #[arg(long)]
        raw_export_requested: bool,
        #[arg(long)]
        provider_assurance: Option<String>,
        #[arg(long)]
        http_host: Option<String>,
        #[arg(long)]
        http_scheme: Option<String>,
        #[arg(long)]
        http_method: Option<String>,
        #[arg(long)]
        http_path: Option<String>,
        #[arg(long)]
        http_request_body_bytes: Option<u64>,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    PlaintextExport {
        #[command(subcommand)]
        command: PlaintextExportCommand,
    },
}

#[derive(Debug, Subcommand)]
enum PlaintextExportCommand {
    Status,
    Enable {
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        mfa_code: Option<String>,
        #[arg(long)]
        allow_without_mfa: bool,
    },
    Disable {
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        mfa_code: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum TicketCommand {
    List,
    Inspect {
        ticket_id: String,
    },
    Issue {
        secret_ref: String,
        #[arg(long, default_value = "cli")]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "terminal_print")]
        delivery_mode: String,
        #[arg(long)]
        raw_export_requested: bool,
        #[arg(long)]
        provider_assurance: Option<String>,
        #[arg(long)]
        http_host: Option<String>,
        #[arg(long)]
        http_scheme: Option<String>,
        #[arg(long)]
        http_method: Option<String>,
        #[arg(long)]
        http_path: Option<String>,
        #[arg(long)]
        http_request_body_bytes: Option<u64>,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Revoke {
        ticket_id: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Validate {
        ticket_id: String,
        #[arg(long)]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "terminal_print")]
        delivery_mode: String,
        #[arg(long)]
        raw_export_requested: bool,
        #[arg(long)]
        provider_assurance: Option<String>,
        #[arg(long)]
        http_host: Option<String>,
        #[arg(long)]
        http_scheme: Option<String>,
        #[arg(long)]
        http_method: Option<String>,
        #[arg(long)]
        http_path: Option<String>,
        #[arg(long)]
        http_request_body_bytes: Option<u64>,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Renew {
        ticket_id: String,
        #[arg(long)]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "terminal_print")]
        delivery_mode: String,
        #[arg(long)]
        raw_export_requested: bool,
        #[arg(long)]
        provider_assurance: Option<String>,
        #[arg(long)]
        http_host: Option<String>,
        #[arg(long)]
        http_scheme: Option<String>,
        #[arg(long)]
        http_method: Option<String>,
        #[arg(long)]
        http_path: Option<String>,
        #[arg(long)]
        http_request_body_bytes: Option<u64>,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Consume {
        ticket_id: String,
        #[arg(long)]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "terminal_print")]
        delivery_mode: String,
        #[arg(long)]
        raw_export_requested: bool,
        #[arg(long)]
        provider_assurance: Option<String>,
        #[arg(long)]
        http_host: Option<String>,
        #[arg(long)]
        http_scheme: Option<String>,
        #[arg(long)]
        http_method: Option<String>,
        #[arg(long)]
        http_path: Option<String>,
        #[arg(long)]
        http_request_body_bytes: Option<u64>,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        allow_plaintext: bool,
        #[arg(long)]
        mfa_code: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum RotationCommand {
    Start {
        secret_ref: String,
        #[arg(long)]
        value: Option<String>,
        #[arg(long)]
        stdin: bool,
        #[arg(long, default_value = "generic")]
        secret_type: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Verify {
        job_id: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Promote {
        job_id: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Rollback {
        job_id: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    List,
}

#[derive(Debug, Subcommand)]
enum ProviderCommand {
    List {
        #[arg(long, default_value = "/dev/tpmrm0")]
        device: String,
    },
    Detect {
        #[arg(long, default_value = "/dev/tpmrm0")]
        device: String,
    },
    TestTpm2 {
        #[arg(long, default_value = "/dev/tpmrm0")]
        device: String,
    },
    TestTpm2Direct {
        #[arg(long, default_value = "/dev/tpmrm0")]
        device: String,
    },
    TestSystemFingerprint,
    TestYubikeyPiv,
    Enroll {
        provider: String,
        #[arg(long)]
        current_passphrase: Option<String>,
        #[arg(long)]
        new_passphrase: Option<String>,
        #[arg(long, default_value = "/dev/tpmrm0")]
        tpm_device: String,
    },
}

#[derive(Debug, Subcommand)]
enum ModelProviderCommand {
    List,
    Setup {
        preset: String,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        secret_ref: Option<String>,
        #[arg(long)]
        policy_id: Option<String>,
        #[arg(long)]
        consumer: Option<String>,
        #[arg(long, default_value = "model.chat")]
        purpose: String,
        #[arg(long, default_value = "model.discovery")]
        model_discovery_purpose: String,
        #[arg(long)]
        upstream_base_url: Option<String>,
        #[arg(long)]
        listen: Option<String>,
        #[arg(long)]
        credential_header: Option<String>,
        #[arg(long)]
        credential_prefix: Option<String>,
        #[arg(long, default_value_t = 10 * 1024 * 1024)]
        max_body_bytes: u64,
        #[arg(long)]
        require_mfa: bool,
        #[arg(long)]
        passphrase: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum MfaCommand {
    EnrollTotp {
        #[arg(long, default_value = "HBSE")]
        issuer: String,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        passphrase: Option<String>,
    },
    VerifyTotp {
        code: String,
        #[arg(long)]
        passphrase: Option<String>,
    },
    Status,
}

#[derive(Debug, Subcommand)]
enum BrokerCommand {
    Status {
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
    },
    Unlock {
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        mfa_code: Option<String>,
    },
    MfaVerify {
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
        code: String,
    },
    Lock {
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
    },
    Checkout {
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
        #[arg(long)]
        secret_ref: String,
        #[arg(long, default_value = "cli")]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "terminal_print")]
        delivery_mode: String,
    },
    Materialize {
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
        #[arg(long)]
        secret_ref: String,
        #[arg(long, default_value = "cli")]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "terminal_print")]
        delivery_mode: String,
        #[arg(long)]
        allow_plaintext: bool,
    },
    ProviderHttp {
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
        #[arg(long)]
        secret_ref: String,
        #[arg(long, default_value = "cli")]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "GET")]
        method: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        header: Vec<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long, default_value = "Authorization")]
        credential_header: String,
        #[arg(long, default_value = "Bearer ")]
        credential_prefix: String,
        #[arg(long, default_value_t = 30.0)]
        timeout_seconds: f64,
        #[arg(long, default_value_t = 10 * 1024 * 1024)]
        max_response_bytes: u64,
    },
    CleanupSocket {
        #[arg(long, default_value_os_t = default_runtime_socket_path())]
        socket: PathBuf,
    },
    InstallService {
        #[arg(long, default_value = "user")]
        scope: String,
        #[arg(long)]
        unit_dir: Option<PathBuf>,
        #[arg(long)]
        socket: Option<String>,
        #[arg(long, default_value_t = 900.0)]
        idle_timeout_seconds: f64,
        #[arg(long)]
        broker_executable: Option<String>,
        #[arg(long)]
        service_user: Option<String>,
        #[arg(long)]
        enable: bool,
        #[arg(long)]
        start: bool,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DotenvCommand {
    Scan {
        path: PathBuf,
    },
    Run {
        path: PathBuf,
        #[arg(long, default_value = "cli")]
        consumer: String,
        #[arg(long)]
        purpose: String,
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ReleaseCommand {
    Evidence {
        #[arg(long, default_value = "release")]
        output_dir: PathBuf,
        #[arg(long, default_value = ".")]
        project_root: PathBuf,
        #[arg(long, default_value = "0.1.0")]
        version: String,
    },
    Keygen {
        #[arg(long)]
        private_key: PathBuf,
        #[arg(long)]
        public_key: PathBuf,
        #[arg(long)]
        encrypted: bool,
        #[arg(long, default_value = "HBSE_RELEASE_KEY_PASSPHRASE")]
        key_passphrase_env: String,
    },
    Sign {
        #[arg(long, default_value = "release")]
        release_dir: PathBuf,
        #[arg(long)]
        private_key: PathBuf,
        #[arg(long)]
        public_key_out: Option<PathBuf>,
        #[arg(long)]
        artifact: Vec<PathBuf>,
        #[arg(long, default_value = "0.1.0")]
        version: String,
        #[arg(long, default_value = "HBSE_RELEASE_KEY_PASSPHRASE")]
        key_passphrase_env: String,
    },
    Verify {
        #[arg(long, default_value = "release")]
        release_dir: PathBuf,
        #[arg(long)]
        public_key: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ReadinessCommand {
    Check {
        #[arg(long, default_value = "A2")]
        target: String,
        #[arg(long, default_value = "release")]
        release_dir: PathBuf,
        #[arg(long)]
        verify_audit: bool,
        #[arg(long)]
        passphrase: Option<String>,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let vault = LocalVault::new(SQLiteVaultStore::new(&cli.vault));
    match cli.command {
        Command::Version => println!("{}", hbse::HBSE_VERSION),
        Command::Vault { command } => match command {
            VaultCommand::Init {
                namespace,
                provider,
                tpm_device,
                passphrase,
            } => {
                let header = match provider.as_str() {
                    "passphrase" => {
                        let passphrase = passphrase_or_env(passphrase)?;
                        vault.init_passphrase(&passphrase, namespace)?
                    }
                    "tpm2" => vault.init_tpm2(namespace, &tpm_device)?,
                    "tpm2-direct" | "tpm2-esapi" => {
                        vault.init_tpm2_esapi(namespace, &tpm_device)?
                    }
                    "system-fingerprint" => vault.init_system_fingerprint(namespace)?,
                    _ => return Err(format!("unsupported provider: {provider}").into()),
                };
                print_vault_status(&header, cli.json)?;
            }
            VaultCommand::Status => {
                let header = vault.status()?;
                print_vault_status(&header, cli.json)?;
            }
            VaultCommand::Backup { destination } => {
                let manifest = create_backup(&vault.store, &destination)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&manifest)?);
                } else {
                    println!("backup created: {}", destination.display());
                    println!("vault_id: {}", manifest.vault_id);
                }
            }
            VaultCommand::Restore { source } => {
                let manifest = restore_backup(&source, vault.store.path())?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&manifest)?);
                } else {
                    println!("backup restored for vault {}", manifest.vault_id);
                }
            }
            VaultCommand::RecoveryCreate {
                destination,
                passphrase,
                recovery_secret,
                mnemonic,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                if mnemonic && recovery_secret.is_some() {
                    return Err("use either --mnemonic or --recovery-secret, not both".into());
                }
                let recovery_secret = if mnemonic {
                    generate_mnemonic_phrase()
                } else {
                    recovery_secret_or_env(recovery_secret)?
                };
                let package = vault.create_recovery_package(Some(&passphrase), &recovery_secret)?;
                package.write(&destination)?;
                if cli.json {
                    let mut output = json!({
                        "recovery_id": package.recovery_id,
                        "vault_id": package.vault_id,
                        "destination": destination,
                        "recovery_secret_format": if mnemonic { "hbse-mnemonic-v1" } else { "secret" },
                    });
                    if mnemonic {
                        output["recovery_mnemonic"] =
                            serde_json::Value::String(recovery_secret.clone());
                    }
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    println!("recovery package created: {}", destination.display());
                    if mnemonic {
                        println!("recovery_mnemonic: {recovery_secret}");
                        println!("Store this mnemonic separately. It is shown only now.");
                    }
                }
            }
            VaultCommand::RecoveryInspect {
                package,
                recovery_secret,
                recovery_mnemonic,
            } => {
                if recovery_secret.is_some() && recovery_mnemonic.is_some() {
                    return Err(
                        "use either --recovery-secret or --recovery-mnemonic, not both".into(),
                    );
                }
                let package = RecoveryPackage::read(package)?;
                let vault_id = vault
                    .status()
                    .ok()
                    .map(|header| header.vault_id)
                    .unwrap_or_default();
                let vault_matches = !vault_id.is_empty() && package.vault_id == vault_id;
                let verification = if let Some(secret) = recovery_mnemonic
                    .map(|value| normalize_mnemonic_phrase(&value))
                    .or(recovery_secret)
                    .or_else(|| env::var("HBSE_RECOVERY_SECRET").ok())
                {
                    match RecoveryManager::default().unwrap_root_key(&package, &secret) {
                        Ok(_) => "verified",
                        Err(_) => "failed",
                    }
                } else {
                    "not_checked"
                };
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "format_version": package.format_version,
                            "recovery_id": package.recovery_id,
                            "vault_id": package.vault_id,
                            "created_at": package.created_at,
                            "vault_matches_current": vault_matches,
                            "secret_verification": verification,
                            "warning": package.warning,
                        }))?
                    );
                } else {
                    println!("recovery_id: {}", package.recovery_id);
                    println!("vault_id: {}", package.vault_id);
                    println!("created_at: {}", package.created_at);
                    println!("vault_matches_current: {vault_matches}");
                    println!("secret_verification: {verification}");
                }
            }
            VaultCommand::Recover {
                package,
                recovery_secret,
                recovery_mnemonic,
                new_provider,
                new_passphrase,
                tpm_device,
            } => {
                if recovery_secret.is_some() && recovery_mnemonic.is_some() {
                    return Err(
                        "use either --recovery-secret or --recovery-mnemonic, not both".into(),
                    );
                }
                let package = RecoveryPackage::read(package)?;
                let recovery_secret = if let Some(mnemonic) = recovery_mnemonic {
                    normalize_mnemonic_phrase(&mnemonic)
                } else {
                    recovery_secret_or_env(recovery_secret)?
                };
                let header = vault.recover_provider_from_package(
                    &package,
                    &recovery_secret,
                    &new_provider,
                    new_passphrase.as_deref(),
                    &tpm_device,
                )?;
                if cli.json {
                    print_vault_status(&header, true)?;
                } else {
                    println!(
                        "recovered vault {} to provider {}",
                        header.vault_id,
                        header
                            .provider_binding
                            .get("provider_id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown")
                    );
                }
            }
        },
        Command::Secret { command } => match command {
            SecretCommand::Put {
                secret_ref,
                value,
                stdin,
                secret_type,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let plaintext = read_secret_value(value, stdin)?;
                let version = vault.put_secret(
                    &secret_ref,
                    plaintext.as_bytes(),
                    &passphrase,
                    parse_secret_type(&secret_type)?,
                )?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "secret_ref": secret_ref,
                            "version": version,
                        }))?
                    );
                } else {
                    println!("stored {secret_ref} version {version}");
                }
            }
            SecretCommand::Get {
                secret_ref,
                passphrase,
                allow_plaintext,
                mfa_code,
            } => {
                let passphrase =
                    plaintext_export_passphrase(&vault, passphrase, allow_plaintext, mfa_code)?;
                let plaintext = vault.get_secret(&secret_ref, &passphrase)?;
                print!("{}", String::from_utf8_lossy(&plaintext));
            }
            SecretCommand::Inspect { secret_ref } => {
                let record = vault.load_latest_secret(&secret_ref)?;
                let summary = secret_record_summary(&record);
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&summary)?);
                } else {
                    println!("secret_ref: {}", record.secret_ref);
                    println!("secret_id: {}", record.secret_id);
                    println!("version: {}", record.secret_version);
                    println!("status: {}", status_string(record.status));
                    println!("secret_type: {}", secret_type_string(record.secret_type));
                    println!("created_at: {}", record.created_at);
                    println!("policy_id: {}", record.policy_id);
                }
            }
            SecretCommand::Disable {
                secret_ref,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let record = vault.disable_secret(&secret_ref, &passphrase)?;
                print_secret_update(&record, "disabled", cli.json)?;
            }
            SecretCommand::Destroy {
                secret_ref,
                passphrase,
                reason,
            } => {
                if reason.trim().is_empty() {
                    return Err("destroy reason must not be empty".into());
                }
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let record = vault.destroy_secret(&secret_ref, &passphrase)?;
                print_secret_update(&record, "destroyed", cli.json)?;
            }
            SecretCommand::List => {
                let summaries = vault.list_secrets()?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&summaries)?);
                } else {
                    for summary in summaries {
                        println!(
                            "{}\t{}\t{}\t{}",
                            summary.secret_ref,
                            summary.latest_version,
                            summary.status,
                            summary.secret_type
                        );
                    }
                }
            }
        },
        Command::Audit { command } => match command {
            AuditCommand::List { event_type, limit } => {
                let mut events = vault.list_audit_events()?;
                if let Some(event_type) = event_type {
                    events.retain(|event| event.event_type == event_type);
                }
                if let Some(limit) = limit {
                    if events.len() > limit {
                        events = events.split_off(events.len() - limit);
                    }
                }
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&events)?);
                } else {
                    for event in events {
                        println!(
                            "{}\t{}\t{}\t{}\t{}",
                            event.timestamp,
                            event.event_type,
                            event.severity,
                            event.decision,
                            event.event_id
                        );
                    }
                }
            }
            AuditCommand::Export {
                destination,
                event_type,
            } => {
                let mut events = vault.list_audit_events()?;
                if let Some(event_type) = event_type {
                    events.retain(|event| event.event_type == event_type);
                }
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(
                    &destination,
                    serde_json::to_string_pretty(&json!({ "events": events }))? + "\n",
                )?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "destination": destination,
                        }))?
                    );
                } else {
                    println!("audit exported: {}", destination.display());
                }
            }
            AuditCommand::Verify { passphrase } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                vault.verify_audit(&passphrase)?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "verified": true,
                        }))?
                    );
                } else {
                    println!("audit chain verified");
                }
            }
        },
        Command::Policy { command } => match command {
            PolicyCommand::Put {
                file,
                stdin,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let raw = read_text_input(file, stdin)?;
                let policy: AccessPolicy = serde_json::from_str(&raw)?;
                let policy = vault.save_policy(policy, &passphrase)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&policy)?);
                } else {
                    println!("saved policy {}", policy.policy_id);
                }
            }
            PolicyCommand::List => {
                let policies = vault.list_policies()?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&policies)?);
                } else {
                    for policy in policies {
                        println!("{}", policy.policy_id);
                    }
                }
            }
            PolicyCommand::Export { destination } => {
                let policies = vault.list_policies()?;
                let output = serde_json::to_string_pretty(&json!({ "policies": policies }))? + "\n";
                if let Some(destination) = destination {
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&destination, output)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "destination": destination,
                            }))?
                        );
                    } else {
                        println!("policy exported: {}", destination.display());
                    }
                } else {
                    print!("{output}");
                }
            }
            PolicyCommand::Hash { file, stdin } => {
                let raw = read_text_input(file, stdin)?;
                let policy: AccessPolicy = serde_json::from_str(&raw)?;
                let hash = hbse::tickets::policy_hash(&policy);
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "policy_id": policy.policy_id,
                            "policy_hash": hash,
                        }))?
                    );
                } else {
                    println!("{hash}");
                }
            }
            PolicyCommand::Test {
                secret_ref,
                consumer,
                purpose,
                delivery_mode,
                raw_export_requested,
                provider_assurance,
                http_host,
                http_scheme,
                http_method,
                http_path,
                http_request_body_bytes,
            } => {
                let request = build_access_request(
                    secret_ref,
                    consumer,
                    purpose,
                    &delivery_mode,
                    raw_export_requested,
                    provider_assurance,
                    http_host,
                    http_scheme,
                    http_method,
                    http_path,
                    http_request_body_bytes,
                )?;
                let (allowed, reason, policy_id) = vault.evaluate_policy(request)?;
                let output = json!({
                    "decision": if allowed { "allow" } else { "deny" },
                    "reason": reason,
                    "policy_id": policy_id,
                });
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    println!(
                        "{}: {}",
                        output["decision"].as_str().unwrap_or("deny"),
                        output["reason"].as_str().unwrap_or("unknown")
                    );
                }
            }
        },
        Command::Config { command } => match command {
            ConfigCommand::PlaintextExport { command } => match command {
                PlaintextExportCommand::Status => {
                    let enabled = vault.plaintext_export_enabled()?;
                    let mfa_enrolled = vault.totp_mfa_enrolled()?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "plaintext_export_enabled": enabled,
                                "totp_mfa_enrolled": mfa_enrolled,
                                "mfa_required_for_plaintext_export": enabled && mfa_enrolled,
                            }))?
                        );
                    } else {
                        println!(
                            "plaintext_export: {}",
                            if enabled { "enabled" } else { "disabled" }
                        );
                        println!(
                            "totp_mfa: {}",
                            if mfa_enrolled {
                                "enrolled"
                            } else {
                                "not enrolled"
                            }
                        );
                        println!(
                            "mfa_required_for_plaintext_export: {}",
                            enabled && mfa_enrolled
                        );
                    }
                }
                PlaintextExportCommand::Enable {
                    passphrase,
                    mfa_code,
                    allow_without_mfa,
                } => {
                    let passphrase = config_change_passphrase(&vault, passphrase, mfa_code)?;
                    vault.set_plaintext_export_enabled(&passphrase, true, allow_without_mfa)?;
                    println!("plaintext export enabled");
                }
                PlaintextExportCommand::Disable {
                    passphrase,
                    mfa_code,
                } => {
                    let passphrase = config_change_passphrase(&vault, passphrase, mfa_code)?;
                    vault.set_plaintext_export_enabled(&passphrase, false, true)?;
                    println!("plaintext export disabled");
                }
            },
        },
        Command::Ticket { command } => match command {
            TicketCommand::List => {
                let tickets = vault.list_tickets()?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&tickets)?);
                } else {
                    for ticket in tickets {
                        println!(
                            "{}\t{}\t{}\t{}\t{}\t{}",
                            ticket.ticket_id,
                            ticket.secret_ref,
                            ticket.consumer,
                            ticket.purpose,
                            delivery_mode_string(ticket.delivery_mode),
                            if ticket.revoked { "revoked" } else { "active" }
                        );
                    }
                }
            }
            TicketCommand::Inspect { ticket_id } => {
                let ticket = vault.load_ticket(&ticket_id)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&ticket)?);
                } else {
                    println!("ticket_id: {}", ticket.ticket_id);
                    println!("secret_ref: {}", ticket.secret_ref);
                    println!("consumer: {}", ticket.consumer);
                    println!("purpose: {}", ticket.purpose);
                    println!(
                        "delivery_mode: {}",
                        delivery_mode_string(ticket.delivery_mode)
                    );
                    println!("uses_remaining: {}", ticket.uses_remaining);
                    println!("revoked: {}", ticket.revoked);
                    println!("expires_at: {}", ticket.expires_at);
                }
            }
            TicketCommand::Issue {
                secret_ref,
                consumer,
                purpose,
                delivery_mode,
                raw_export_requested,
                provider_assurance,
                http_host,
                http_scheme,
                http_method,
                http_path,
                http_request_body_bytes,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let request = build_access_request(
                    secret_ref,
                    consumer,
                    purpose,
                    &delivery_mode,
                    raw_export_requested,
                    provider_assurance,
                    http_host,
                    http_scheme,
                    http_method,
                    http_path,
                    http_request_body_bytes,
                )?;
                let ticket = vault.issue_ticket(request, &passphrase)?;
                println!("{}", serde_json::to_string_pretty(&ticket)?);
            }
            TicketCommand::Revoke {
                ticket_id,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let ticket = vault.revoke_ticket(&ticket_id, &passphrase)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&ticket)?);
                } else {
                    println!("revoked ticket {}", ticket.ticket_id);
                }
            }
            TicketCommand::Validate {
                ticket_id,
                consumer,
                purpose,
                delivery_mode,
                raw_export_requested,
                provider_assurance,
                http_host,
                http_scheme,
                http_method,
                http_path,
                http_request_body_bytes,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let ticket = vault.load_ticket(&ticket_id)?;
                let request = build_access_request(
                    ticket.secret_ref,
                    consumer,
                    purpose,
                    &delivery_mode,
                    raw_export_requested,
                    provider_assurance,
                    http_host,
                    http_scheme,
                    http_method,
                    http_path,
                    http_request_body_bytes,
                )?;
                let ticket = vault.validate_ticket(&ticket_id, request, &passphrase)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&ticket)?);
                } else {
                    println!("ticket {} valid", ticket.ticket_id);
                }
            }
            TicketCommand::Renew {
                ticket_id,
                consumer,
                purpose,
                delivery_mode,
                raw_export_requested,
                provider_assurance,
                http_host,
                http_scheme,
                http_method,
                http_path,
                http_request_body_bytes,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let ticket = vault.load_ticket(&ticket_id)?;
                let request = build_access_request(
                    ticket.secret_ref,
                    consumer,
                    purpose,
                    &delivery_mode,
                    raw_export_requested,
                    provider_assurance,
                    http_host,
                    http_scheme,
                    http_method,
                    http_path,
                    http_request_body_bytes,
                )?;
                let ticket = vault.renew_ticket(&ticket_id, request, &passphrase)?;
                println!("{}", serde_json::to_string_pretty(&ticket)?);
            }
            TicketCommand::Consume {
                ticket_id,
                consumer,
                purpose,
                delivery_mode,
                raw_export_requested: _raw_export_requested,
                provider_assurance,
                http_host,
                http_scheme,
                http_method,
                http_path,
                http_request_body_bytes,
                passphrase,
                allow_plaintext,
                mfa_code,
            } => {
                let passphrase =
                    plaintext_export_passphrase(&vault, passphrase, allow_plaintext, mfa_code)?;
                let ticket = vault.load_ticket(&ticket_id)?;
                let request = build_access_request(
                    ticket.secret_ref,
                    consumer,
                    purpose,
                    &delivery_mode,
                    true,
                    provider_assurance,
                    http_host,
                    http_scheme,
                    http_method,
                    http_path,
                    http_request_body_bytes,
                )?;
                let plaintext =
                    vault.consume_ticket_for_secret(&ticket_id, request, &passphrase)?;
                print!("{}", String::from_utf8_lossy(&plaintext));
            }
        },
        Command::Rotation { command } => match command {
            RotationCommand::Start {
                secret_ref,
                value,
                stdin,
                secret_type,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let plaintext = read_secret_value(value, stdin)?;
                let job = vault.start_rotation(
                    &secret_ref,
                    plaintext.as_bytes(),
                    &passphrase,
                    parse_secret_type(&secret_type)?,
                )?;
                print_rotation_job(&job, cli.json)?;
            }
            RotationCommand::Verify { job_id, passphrase } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let job = vault.verify_rotation(&job_id, &passphrase)?;
                print_rotation_job(&job, cli.json)?;
            }
            RotationCommand::Promote { job_id, passphrase } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let job = vault.promote_rotation(&job_id, &passphrase)?;
                print_rotation_job(&job, cli.json)?;
            }
            RotationCommand::Rollback { job_id, passphrase } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let job = vault.rollback_rotation(&job_id, &passphrase)?;
                print_rotation_job(&job, cli.json)?;
            }
            RotationCommand::List => {
                let jobs = vault.list_rotation_jobs()?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&jobs)?);
                } else {
                    for job in jobs {
                        println!(
                            "{}\t{}\t{}\t{}",
                            job.job_id,
                            job.secret_ref,
                            job.staged_version,
                            rotation_state_string(job.status)
                        );
                    }
                }
            }
        },
        Command::Provider { command } => match command {
            ProviderCommand::List { device } => {
                let providers = local_provider_catalog(&device);
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&providers)?);
                } else {
                    for provider in providers {
                        let binding = if provider.vault_binding_supported {
                            "vault-binding"
                        } else {
                            "detect-only"
                        };
                        let availability = if provider.available {
                            "available"
                        } else {
                            "unavailable"
                        };
                        println!(
                            "{}\t{}\t{}\t{}\t{}",
                            provider.provider_id,
                            provider.assurance_level,
                            binding,
                            availability,
                            provider.detail
                        );
                    }
                }
            }
            ProviderCommand::Detect { device } => {
                let status = LinuxTpm2ToolsProvider::new(device).detect();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!("{}", status.detail);
                }
                if !status.available {
                    std::process::exit(4);
                }
            }
            ProviderCommand::TestTpm2 { device } => {
                let status = LinuxTpm2ToolsProvider::new(device).self_test()?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!("{}", status.detail);
                }
                if !status.available {
                    std::process::exit(4);
                }
            }
            ProviderCommand::TestTpm2Direct { device } => {
                let status = LinuxTpm2EsapiProvider::new(device).self_test()?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!("{}", status.detail);
                }
                if !status.available {
                    std::process::exit(4);
                }
            }
            ProviderCommand::TestSystemFingerprint => {
                let status = SystemFingerprintProvider::default().detect();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!("{}", status.detail);
                }
                if !status.available {
                    std::process::exit(4);
                }
            }
            ProviderCommand::TestYubikeyPiv => {
                let status = YubikeyPivProvider::detect();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!("{}", status.detail);
                }
                if !status.available {
                    std::process::exit(4);
                }
            }
            ProviderCommand::Enroll {
                provider,
                current_passphrase,
                new_passphrase,
                tpm_device,
            } => {
                let current = unlock_passphrase(&vault, current_passphrase)?;
                let header = vault.rewrap_provider(
                    Some(&current),
                    &provider,
                    new_passphrase.as_deref(),
                    &tpm_device,
                )?;
                print_vault_status(&header, cli.json)?;
            }
        },
        Command::ModelProvider { command } => match command {
            ModelProviderCommand::List => {
                let presets = model_provider_presets();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&presets)?);
                } else {
                    for preset in presets {
                        println!(
                            "{}\t{}\t{}\t{}",
                            preset.id, preset.base_url, preset.credential_header, preset.note
                        );
                    }
                }
            }
            ModelProviderCommand::Setup {
                preset,
                api_key_env,
                stdin,
                secret_ref,
                policy_id,
                consumer,
                purpose,
                model_discovery_purpose,
                upstream_base_url,
                listen,
                credential_header,
                credential_prefix,
                max_body_bytes,
                require_mfa,
                passphrase,
            } => {
                let preset = model_provider_preset(&preset)
                    .ok_or_else(|| format!("unknown model provider preset: {preset}"))?;
                let secret_ref = secret_ref
                    .unwrap_or_else(|| format!("secret://providers/{}/api-key", preset.id));
                let policy_id =
                    policy_id.unwrap_or_else(|| format!("model-provider-{}", preset.id));
                let consumer =
                    consumer.unwrap_or_else(|| format!("hbse.http-gateway.{}", preset.id));
                let base_url = upstream_base_url.unwrap_or_else(|| preset.base_url.to_string());
                let credential_header =
                    credential_header.unwrap_or_else(|| preset.credential_header.to_string());
                let credential_prefix =
                    credential_prefix.unwrap_or_else(|| preset.credential_prefix.to_string());
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let api_key = read_model_provider_key(api_key_env.as_deref(), stdin)?;
                vault.put_secret(
                    &secret_ref,
                    api_key.trim_end_matches(['\r', '\n']).as_bytes(),
                    &passphrase,
                    SecretType::ApiKey,
                )?;
                let host = http_host_from_base_url(&base_url)?;
                let path_prefix = http_path_prefix_from_base_url(&base_url);
                let policy = AccessPolicy {
                    policy_id: policy_id.clone(),
                    secret_refs: vec![secret_ref.clone()],
                    allowed_consumers: vec![consumer.clone()],
                    denied_consumers: vec![],
                    allowed_purposes: vec![purpose.clone(), model_discovery_purpose.clone()],
                    denied_purposes: vec![],
                    allowed_delivery_modes: vec![DeliveryMode::BrokeredHttp],
                    allowed_http_hosts: vec![host],
                    denied_http_hosts: vec![],
                    allowed_http_methods: vec![
                        "GET".to_string(),
                        "POST".to_string(),
                        "DELETE".to_string(),
                    ],
                    denied_http_methods: vec![],
                    allowed_http_path_prefixes: vec![path_prefix],
                    denied_http_path_prefixes: vec![],
                    require_https_for_brokered_http: base_url.starts_with("https://"),
                    max_http_request_body_bytes: Some(max_body_bytes),
                    allowed_os_uids: vec![],
                    denied_os_uids: vec![],
                    allowed_executable_paths: vec![],
                    denied_executable_paths: vec![],
                    allowed_executable_sha256: vec![],
                    denied_executable_sha256: vec![],
                    exportable: false,
                    max_ticket_ttl_seconds: 60,
                    max_uses: 1,
                    minimum_provider_assurance: "A1".to_string(),
                    require_mfa,
                    expires_at: None,
                };
                vault.save_policy(policy, &passphrase)?;
                let listen = listen.unwrap_or_else(|| "127.0.0.1:8787".to_string());
                let gateway_command = model_provider_gateway_command(
                    &base_url,
                    &secret_ref,
                    &consumer,
                    &purpose,
                    &model_discovery_purpose,
                    &credential_header,
                    &credential_prefix,
                    &listen,
                );
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "provider": preset.id,
                            "secret_ref": secret_ref,
                            "policy_id": policy_id,
                            "consumer": consumer,
                            "upstream_base_url": base_url,
                            "local_base_url": format!("http://{listen}/v1"),
                            "gateway_command": gateway_command,
                        }))?
                    );
                } else {
                    println!("model provider configured: {}", preset.id);
                    println!("secret_ref: {secret_ref}");
                    println!("policy_id: {policy_id}");
                    println!("consumer: {consumer}");
                    println!("local_base_url: http://{listen}/v1");
                    println!("gateway command:");
                    println!("{gateway_command}");
                    println!("set unmodified OpenAI-compatible clients to:");
                    println!("OPENAI_BASE_URL=http://{listen}/v1");
                    println!("OPENAI_API_KEY=hbse-placeholder");
                }
            }
        },
        Command::Mfa { command } => match command {
            MfaCommand::EnrollTotp {
                issuer,
                account,
                passphrase,
            } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                let account = account.unwrap_or_else(|| {
                    vault
                        .status()
                        .map(|header| header.namespace_id)
                        .unwrap_or_else(|_| "local-vault".to_string())
                });
                let enrollment = vault.enroll_totp_mfa(&passphrase, &issuer, &account)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&enrollment)?);
                } else {
                    println!("TOTP MFA enrolled");
                    println!("issuer: {}", enrollment.issuer);
                    println!("account: {}", enrollment.account);
                    println!("secret_base32: {}", enrollment.secret_base32);
                    println!("otpauth_uri: {}", enrollment.otpauth_uri);
                }
            }
            MfaCommand::VerifyTotp { code, passphrase } => {
                let passphrase = unlock_passphrase(&vault, passphrase)?;
                vault.verify_totp_mfa(&passphrase, &code)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&json!({"ok": true}))?);
                } else {
                    println!("TOTP MFA verified");
                }
            }
            MfaCommand::Status => {
                let enrolled = vault.totp_mfa_enrolled()?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "totp_enrolled": enrolled,
                        }))?
                    );
                } else if enrolled {
                    println!("TOTP MFA enrolled");
                } else {
                    println!("TOTP MFA not enrolled");
                }
            }
        },
        Command::Broker { command } => match command {
            BrokerCommand::Status { socket } => {
                print_broker_response(broker_daemon::request(
                    socket,
                    &json!({"command": "status"}),
                )?)?;
            }
            BrokerCommand::Unlock {
                socket,
                passphrase,
                mfa_code,
            } => {
                let response = broker_daemon::request(
                    socket,
                    &json!({
                        "command": "unlock",
                        "passphrase": passphrase.or_else(|| env::var("HBSE_PASSPHRASE").ok()),
                        "mfa_code": mfa_code,
                    }),
                )?;
                print_broker_response(response)?;
            }
            BrokerCommand::MfaVerify { socket, code } => {
                print_broker_response(broker_daemon::request(
                    socket,
                    &json!({
                        "command": "mfa_verify",
                        "mfa_code": code,
                    }),
                )?)?;
            }
            BrokerCommand::Lock { socket } => {
                print_broker_response(broker_daemon::request(
                    socket,
                    &json!({"command": "lock"}),
                )?)?;
            }
            BrokerCommand::Checkout {
                socket,
                secret_ref,
                consumer,
                purpose,
                delivery_mode,
            } => {
                print_broker_response(broker_daemon::request(
                    socket,
                    &json!({
                        "command": "checkout",
                        "secret_ref": secret_ref,
                        "consumer": consumer,
                        "purpose": purpose,
                        "delivery_mode": delivery_mode,
                    }),
                )?)?;
            }
            BrokerCommand::Materialize {
                socket,
                secret_ref,
                consumer,
                purpose,
                delivery_mode,
                allow_plaintext,
            } => {
                require_plaintext_export_confirmation(allow_plaintext)?;
                print_broker_response(broker_daemon::request(
                    socket,
                    &json!({
                        "command": "materialize",
                        "secret_ref": secret_ref,
                        "consumer": consumer,
                        "purpose": purpose,
                        "delivery_mode": delivery_mode,
                        "raw_export_requested": true,
                    }),
                )?)?;
            }
            BrokerCommand::ProviderHttp {
                socket,
                secret_ref,
                consumer,
                purpose,
                method,
                url,
                header,
                body,
                credential_header,
                credential_prefix,
                timeout_seconds,
                max_response_bytes,
            } => {
                print_broker_response(broker_daemon::request(
                    socket,
                    &json!({
                        "command": "provider_http",
                        "secret_ref": secret_ref,
                        "consumer": consumer,
                        "purpose": purpose,
                        "method": method,
                        "url": url,
                        "headers": parse_headers(header)?,
                        "body": body,
                        "credential_header": credential_header,
                        "credential_prefix": credential_prefix,
                        "timeout_seconds": timeout_seconds,
                        "max_response_bytes": max_response_bytes,
                    }),
                )?)?;
            }
            BrokerCommand::CleanupSocket { socket } => {
                let result = cleanup_broker_socket(&socket)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!(
                        "{}",
                        result["detail"].as_str().unwrap_or("broker socket checked")
                    );
                }
            }
            BrokerCommand::InstallService {
                scope,
                unit_dir,
                socket,
                idle_timeout_seconds,
                broker_executable,
                service_user,
                enable,
                start,
                dry_run,
            } => {
                let result = install_broker_service(BrokerServiceInstallOptions {
                    scope,
                    unit_dir,
                    broker_executable,
                    vault_path: cli.vault.to_string_lossy().to_string(),
                    socket_path: socket,
                    idle_timeout_seconds,
                    service_user,
                    enable,
                    start,
                    dry_run,
                })?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        },
        Command::Dotenv { command } => match command {
            DotenvCommand::Scan { path } => {
                let findings = scan_dotenv(path)?;
                if findings.is_empty() {
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({"findings": []}))?
                        );
                    } else {
                        println!("dotenv: ok");
                    }
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({"findings": findings}))?
                    );
                    if findings
                        .iter()
                        .any(|finding| finding.kind == "likely_raw_secret")
                    {
                        std::process::exit(8);
                    }
                }
            }
            DotenvCommand::Run {
                path,
                consumer,
                purpose,
                command,
            } => {
                let command = strip_separator(command);
                if command.is_empty() {
                    return Err("use hbse dotenv run <file> -- <command>".into());
                }
                let findings = scan_dotenv(&path)?;
                let raw_findings = findings
                    .iter()
                    .filter(|finding| finding.kind == "likely_raw_secret")
                    .cloned()
                    .collect::<Vec<_>>();
                if !raw_findings.is_empty() {
                    eprintln!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "error": "raw secrets detected",
                            "findings": raw_findings,
                        }))?
                    );
                    std::process::exit(8);
                }
                let (plain, refs) = split_dotenv_values(parse_dotenv(&path)?)?;
                let passphrase = unlock_passphrase(&vault, None)?;
                let mut child_env = std::collections::BTreeMap::new();
                child_env.extend(plain);
                for (env_name, secret_ref) in refs {
                    let request = materialization_request(
                        &vault,
                        secret_ref,
                        &consumer,
                        &purpose,
                        DeliveryMode::ChildEnv,
                        false,
                    )?;
                    let ticket = vault.issue_ticket(request.clone(), &passphrase)?;
                    let secret =
                        vault.consume_ticket_for_secret(&ticket.ticket_id, request, &passphrase)?;
                    child_env.insert(env_name, String::from_utf8_lossy(&secret).to_string());
                }
                let exit_code = exec_child(command, child_env, None)?;
                std::process::exit(exit_code);
            }
        },
        Command::Release { command } => match command {
            ReleaseCommand::Evidence {
                output_dir,
                project_root,
                version,
            } => {
                let evidence = generate_release_evidence(output_dir, project_root, &version)?;
                println!("{}", serde_json::to_string_pretty(&evidence)?);
            }
            ReleaseCommand::Keygen {
                private_key,
                public_key,
                encrypted,
                key_passphrase_env,
            } => {
                let passphrase = if encrypted {
                    Some(env::var(&key_passphrase_env).map_err(|_| {
                        format!("{key_passphrase_env} required when --encrypted is set")
                    })?)
                } else {
                    None
                };
                let result =
                    generate_signing_keypair(private_key, public_key, passphrase.as_deref())?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ReleaseCommand::Sign {
                release_dir,
                private_key,
                public_key_out,
                artifact,
                version,
                key_passphrase_env,
            } => {
                let key_passphrase = env::var(&key_passphrase_env).ok();
                let (manifest, signature) = sign_release_artifacts(
                    release_dir,
                    &artifact,
                    private_key,
                    public_key_out,
                    key_passphrase.as_deref(),
                    &version,
                )?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "mode": signature.mode,
                        "manifest": signature.manifest,
                        "public_key_sha256": signature.public_key_sha256,
                        "artifact_count": manifest.artifacts.len(),
                    }))?
                );
            }
            ReleaseCommand::Verify {
                release_dir,
                public_key,
            } => {
                let verification = verify_release_evidence(release_dir, public_key);
                println!("{}", serde_json::to_string_pretty(&verification)?);
                if !verification.passed {
                    std::process::exit(1);
                }
            }
        },
        Command::Run {
            consumer,
            purpose,
            secret_env,
            secret_file_env,
            secret_fd_env,
            secret_stdin,
            env,
            mfa_code,
            command,
        } => {
            let command = strip_separator(command);
            if command.is_empty() {
                return Err("use hbse run --purpose <purpose> -- <command>".into());
            }
            let passphrase = unlock_passphrase(&vault, None)?;
            let mfa_verified = if let Some(code) = mfa_code {
                vault.verify_totp_mfa(&passphrase, &code)?;
                true
            } else {
                false
            };
            let mut child_env = parse_env_assignments(env)?;
            for (env_name, secret_ref) in parse_env_assignments(secret_env)? {
                if !secret_ref.starts_with("secret://") {
                    return Err(format!(
                        "--secret-env {env_name}=... must use a secret:// reference"
                    )
                    .into());
                }
                let request = materialization_request(
                    &vault,
                    secret_ref,
                    &consumer,
                    &purpose,
                    DeliveryMode::ChildEnv,
                    mfa_verified,
                )?;
                let ticket = vault.issue_ticket(request.clone(), &passphrase)?;
                let secret =
                    vault.consume_ticket_for_secret(&ticket.ticket_id, request, &passphrase)?;
                child_env.insert(env_name, String::from_utf8_lossy(&secret).to_string());
            }
            let mut temp_files = Vec::new();
            let mut inherited_files = Vec::new();
            for (env_name, secret_ref) in parse_env_assignments(secret_file_env)? {
                if !secret_ref.starts_with("secret://") {
                    return Err(format!(
                        "--secret-file-env {env_name}=... must use a secret:// reference"
                    )
                    .into());
                }
                let request = materialization_request(
                    &vault,
                    secret_ref,
                    &consumer,
                    &purpose,
                    DeliveryMode::TempFile,
                    mfa_verified,
                )?;
                let ticket = vault.issue_ticket(request.clone(), &passphrase)?;
                let secret =
                    vault.consume_ticket_for_secret(&ticket.ticket_id, request, &passphrase)?;
                let mut temp_file = tempfile::Builder::new().prefix("hbse-secret-").tempfile()?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    temp_file
                        .as_file()
                        .set_permissions(fs::Permissions::from_mode(0o600))?;
                }
                temp_file.write_all(&secret)?;
                temp_file.flush()?;
                child_env.insert(env_name, temp_file.path().to_string_lossy().to_string());
                temp_files.push(temp_file);
            }
            for (env_name, secret_ref) in parse_env_assignments(secret_fd_env)? {
                if !secret_ref.starts_with("secret://") {
                    return Err(format!(
                        "--secret-fd-env {env_name}=... must use a secret:// reference"
                    )
                    .into());
                }
                let request = materialization_request(
                    &vault,
                    secret_ref,
                    &consumer,
                    &purpose,
                    DeliveryMode::Fd,
                    mfa_verified,
                )?;
                let ticket = vault.issue_ticket(request.clone(), &passphrase)?;
                let secret =
                    vault.consume_ticket_for_secret(&ticket.ticket_id, request, &passphrase)?;
                let mut temp_file = tempfile::Builder::new()
                    .prefix("hbse-secret-fd-")
                    .tempfile()?;
                temp_file.write_all(&secret)?;
                temp_file.flush()?;
                let mut file = temp_file.reopen()?;
                file.seek(SeekFrom::Start(0))?;
                make_fd_inheritable(&file)?;
                #[cfg(unix)]
                {
                    use std::os::fd::AsRawFd;
                    child_env.insert(env_name, file.as_raw_fd().to_string());
                }
                #[cfg(not(unix))]
                {
                    return Err("fd delivery is only supported on Unix".into());
                }
                inherited_files.push(file);
                temp_files.push(temp_file);
            }
            let stdin_secret = if let Some(secret_ref) = secret_stdin {
                if !secret_ref.starts_with("secret://") {
                    return Err("--secret-stdin must use a secret:// reference".into());
                }
                let request = materialization_request(
                    &vault,
                    secret_ref,
                    &consumer,
                    &purpose,
                    DeliveryMode::Pipe,
                    mfa_verified,
                )?;
                let ticket = vault.issue_ticket(request.clone(), &passphrase)?;
                Some(vault.consume_ticket_for_secret(&ticket.ticket_id, request, &passphrase)?)
            } else {
                None
            };
            let exit_code = exec_child(command, child_env, stdin_secret)?;
            drop(inherited_files);
            drop(temp_files);
            std::process::exit(exit_code);
        }
        Command::Resolve {
            secret_ref,
            passphrase,
            broker,
            socket,
            consumer,
            purpose,
            delivery_mode,
            allow_plaintext,
            mfa_code,
        } => {
            if !secret_ref.starts_with("secret://") {
                return Err("resolve requires a secret:// reference".into());
            }
            if broker {
                require_plaintext_export_confirmation(allow_plaintext)?;
                let response = broker_daemon::request(
                    socket,
                    &json!({
                        "command": "materialize",
                        "secret_ref": secret_ref,
                        "consumer": consumer,
                        "purpose": purpose,
                        "delivery_mode": delivery_mode,
                        "raw_export_requested": true,
                    }),
                )?;
                if !response
                    .get("ok")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    print_broker_response(response)?;
                    std::process::exit(6);
                }
                let secret = response
                    .get("secret")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("broker response did not include a secret")?;
                print!("{secret}");
            } else {
                let passphrase =
                    plaintext_export_passphrase(&vault, passphrase, allow_plaintext, mfa_code)?;
                let plaintext = vault.get_secret(&secret_ref, &passphrase)?;
                print!("{}", String::from_utf8_lossy(&plaintext));
            }
        }
        Command::Doctor => {
            let report = doctor_report(&vault)?;
            print_checks(&report, cli.json)?;
        }
        Command::Setup { tpm_device } => {
            let report = setup_report(&vault, &cli.vault, &tpm_device)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_setup_report(&report)?;
            }
        }
        Command::Lockdown { reason, passphrase } => {
            let passphrase = unlock_passphrase(&vault, passphrase)?;
            let count = vault.revoke_all_tickets(&passphrase, &reason)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "revoked_tickets": count,
                        "reason": reason,
                    }))?
                );
            } else {
                println!("Revoked {count} ticket(s)");
            }
        }
        Command::Readiness { command } => match command {
            ReadinessCommand::Check {
                target,
                release_dir,
                verify_audit,
                passphrase,
            } => {
                let report =
                    readiness_report(&vault, &target, release_dir, verify_audit, passphrase)?;
                println!("{}", serde_json::to_string_pretty(&report)?);
                if !report["passed"].as_bool().unwrap_or(false) {
                    std::process::exit(1);
                }
            }
        },
    }
    Ok(())
}

fn print_broker_response(value: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn print_secret_update(
    record: &hbse::records::SecretRecord,
    action: &str,
    as_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "secret_ref": record.secret_ref,
                "version": record.secret_version,
                "status": status_string(record.status),
            }))?
        );
    } else {
        println!(
            "{action} {} version {}",
            record.secret_ref, record.secret_version
        );
    }
    Ok(())
}

fn print_rotation_job(
    job: &hbse::rotation::RotationJob,
    as_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(job)?);
    } else {
        println!(
            "{} {} version {} {}",
            job.job_id,
            job.secret_ref,
            job.staged_version,
            rotation_state_string(job.status)
        );
    }
    Ok(())
}

fn passphrase_or_env(value: Option<String>) -> Result<String, Box<dyn std::error::Error>> {
    match value.or_else(|| env::var("HBSE_PASSPHRASE").ok()) {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err("passphrase required; pass --passphrase or set HBSE_PASSPHRASE".into()),
    }
}

fn recovery_secret_or_env(value: Option<String>) -> Result<String, Box<dyn std::error::Error>> {
    match value.or_else(|| env::var("HBSE_RECOVERY_SECRET").ok()) {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err(
            "recovery secret required; pass --recovery-secret or set HBSE_RECOVERY_SECRET".into(),
        ),
    }
}

fn require_plaintext_export_confirmation(
    allow_plaintext: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if allow_plaintext {
        Ok(())
    } else {
        Err("plaintext export requires --allow-plaintext; prefer hbse run, hbse dotenv run, or broker provider-http when possible".into())
    }
}

#[derive(Debug, Clone, Serialize)]
struct ModelProviderPreset {
    id: &'static str,
    display_name: &'static str,
    base_url: &'static str,
    credential_header: &'static str,
    credential_prefix: &'static str,
    note: &'static str,
}

fn model_provider_presets() -> Vec<ModelProviderPreset> {
    vec![
        ModelProviderPreset {
            id: "openai",
            display_name: "OpenAI",
            base_url: "https://api.openai.com/v1",
            credential_header: "Authorization",
            credential_prefix: "Bearer ",
            note: "OpenAI-compatible",
        },
        ModelProviderPreset {
            id: "xai",
            display_name: "xAI",
            base_url: "https://api.x.ai/v1",
            credential_header: "Authorization",
            credential_prefix: "Bearer ",
            note: "OpenAI-compatible",
        },
        ModelProviderPreset {
            id: "openrouter",
            display_name: "OpenRouter",
            base_url: "https://openrouter.ai/api/v1",
            credential_header: "Authorization",
            credential_prefix: "Bearer ",
            note: "OpenAI-compatible",
        },
        ModelProviderPreset {
            id: "groq",
            display_name: "Groq",
            base_url: "https://api.groq.com/openai/v1",
            credential_header: "Authorization",
            credential_prefix: "Bearer ",
            note: "OpenAI-compatible",
        },
        ModelProviderPreset {
            id: "mistral",
            display_name: "Mistral AI",
            base_url: "https://api.mistral.ai/v1",
            credential_header: "Authorization",
            credential_prefix: "Bearer ",
            note: "OpenAI-compatible",
        },
        ModelProviderPreset {
            id: "deepseek",
            display_name: "DeepSeek",
            base_url: "https://api.deepseek.com/v1",
            credential_header: "Authorization",
            credential_prefix: "Bearer ",
            note: "OpenAI-compatible",
        },
        ModelProviderPreset {
            id: "together",
            display_name: "Together AI",
            base_url: "https://api.together.xyz/v1",
            credential_header: "Authorization",
            credential_prefix: "Bearer ",
            note: "OpenAI-compatible",
        },
        ModelProviderPreset {
            id: "perplexity",
            display_name: "Perplexity",
            base_url: "https://api.perplexity.ai",
            credential_header: "Authorization",
            credential_prefix: "Bearer ",
            note: "OpenAI-compatible chat endpoint",
        },
        ModelProviderPreset {
            id: "azure-openai",
            display_name: "Azure OpenAI",
            base_url: "https://example.openai.azure.com/openai",
            credential_header: "api-key",
            credential_prefix: "",
            note: "override --upstream-base-url for your Azure resource/deployment/API version",
        },
        ModelProviderPreset {
            id: "anthropic",
            display_name: "Anthropic",
            base_url: "https://api.anthropic.com/v1",
            credential_header: "x-api-key",
            credential_prefix: "",
            note: "not OpenAI-compatible; use provider-http or a protocol adapter",
        },
        ModelProviderPreset {
            id: "amazon-bedrock",
            display_name: "Amazon Bedrock",
            base_url: "https://bedrock-runtime.us-east-1.amazonaws.com",
            credential_header: "Authorization",
            credential_prefix: "",
            note: "not OpenAI-compatible; AWS SigV4 support requires a protocol adapter",
        },
    ]
}

fn model_provider_preset(id: &str) -> Option<ModelProviderPreset> {
    model_provider_presets()
        .into_iter()
        .find(|preset| preset.id == id)
}

fn read_model_provider_key(
    api_key_env: Option<&str>,
    stdin: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    if stdin {
        let mut value = String::new();
        io::stdin().read_to_string(&mut value)?;
        if value.trim().is_empty() {
            return Err("stdin did not contain an API key".into());
        }
        return Ok(value);
    }
    if let Some(name) = api_key_env {
        let value =
            env::var(name).map_err(|_| format!("environment variable {name} is not set"))?;
        if value.trim().is_empty() {
            return Err(format!("environment variable {name} is empty").into());
        }
        return Ok(value);
    }
    Err("provide the provider key with --api-key-env NAME or --stdin".into())
}

fn http_host_from_base_url(base_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let without_scheme = base_url
        .strip_prefix("https://")
        .or_else(|| base_url.strip_prefix("http://"))
        .ok_or("upstream base URL must start with http:// or https://")?;
    let host = without_scheme
        .split('/')
        .next()
        .ok_or("upstream base URL must include a host")?;
    if host.is_empty() {
        return Err("upstream base URL must include a host".into());
    }
    Ok(host.to_string())
}

fn http_path_prefix_from_base_url(base_url: &str) -> String {
    let without_scheme = base_url
        .strip_prefix("https://")
        .or_else(|| base_url.strip_prefix("http://"))
        .unwrap_or(base_url);
    let mut parts = without_scheme.splitn(2, '/');
    let _host = parts.next();
    let path = parts.next().unwrap_or("");
    if path.is_empty() {
        "/v1/".to_string()
    } else {
        format!("/{}/", path.trim_matches('/'))
    }
}

fn model_provider_gateway_command(
    base_url: &str,
    secret_ref: &str,
    consumer: &str,
    purpose: &str,
    model_discovery_purpose: &str,
    credential_header: &str,
    credential_prefix: &str,
    listen: &str,
) -> String {
    format!(
        "hbse-broker --vault \"$HOME/.local/share/hbse/vault.db\" --socket \"$XDG_RUNTIME_DIR/hbse/broker.sock\" --idle-timeout-seconds 900 --http-listen {listen} --http-upstream-base-url {base_url} --http-secret-ref {secret_ref} --http-consumer {consumer} --http-purpose {purpose} --http-model-discovery-purpose {model_discovery_purpose} --http-credential-header '{}' --http-credential-prefix '{}'",
        shell_single_quote(credential_header),
        shell_single_quote(credential_prefix)
    )
}

fn shell_single_quote(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn plaintext_export_passphrase(
    vault: &LocalVault,
    passphrase: Option<String>,
    allow_plaintext: bool,
    mfa_code: Option<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    require_plaintext_export_confirmation(allow_plaintext)?;
    if !vault.plaintext_export_enabled()? {
        return Err(
            "plaintext export is disabled; enable it with `hbse config plaintext-export enable`"
                .into(),
        );
    }
    let passphrase = unlock_passphrase(vault, passphrase)?;
    if vault.totp_mfa_enrolled()? {
        let code =
            mfa_code.ok_or("plaintext export requires --mfa-code when TOTP MFA is enrolled")?;
        vault.verify_totp_mfa(&passphrase, &code)?;
    }
    Ok(passphrase)
}

fn config_change_passphrase(
    vault: &LocalVault,
    passphrase: Option<String>,
    mfa_code: Option<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    let passphrase = unlock_passphrase(vault, passphrase)?;
    if vault.totp_mfa_enrolled()? {
        let code = mfa_code.ok_or("config change requires --mfa-code when TOTP MFA is enrolled")?;
        vault.verify_totp_mfa(&passphrase, &code)?;
    }
    Ok(passphrase)
}

fn unlock_passphrase(
    vault: &LocalVault,
    value: Option<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    let header = vault.status()?;
    let provider_id = header
        .provider_binding
        .get("provider_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if provider_id == TPM2_PROVIDER_ID || provider_id == TPM2_ESAPI_PROVIDER_ID {
        return Ok(String::new());
    }
    if provider_id == SYSTEM_FINGERPRINT_PROVIDER_ID {
        return Ok(String::new());
    }
    if provider_id == PASSPHRASE_PROVIDER_ID {
        return passphrase_or_env(value);
    }
    Err(format!("unsupported provider: {provider_id}").into())
}

fn read_secret_value(
    value: Option<String>,
    read_stdin: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    match (value, read_stdin) {
        (Some(_), true) => Err("use either --value or --stdin, not both".into()),
        (Some(value), false) => Ok(value),
        (None, true) => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            Ok(buffer)
        }
        (None, false) => Err("secret value required; pass --value or --stdin".into()),
    }
}

fn read_text_input(
    file: Option<PathBuf>,
    read_stdin: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    match (file, read_stdin) {
        (Some(_), true) => Err("use either --file or --stdin, not both".into()),
        (Some(path), false) => Ok(std::fs::read_to_string(path)?),
        (None, true) => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            Ok(buffer)
        }
        (None, false) => Err("input required; pass --file or --stdin".into()),
    }
}

fn parse_headers(
    values: Vec<String>,
) -> Result<serde_json::Map<String, serde_json::Value>, Box<dyn std::error::Error>> {
    let mut headers = serde_json::Map::new();
    for value in values {
        let Some((key, header_value)) = value.split_once(':') else {
            return Err(format!("header must be KEY:VALUE: {value}").into());
        };
        headers.insert(
            key.trim().to_string(),
            serde_json::Value::String(header_value.trim().to_string()),
        );
    }
    Ok(headers)
}

fn parse_secret_type(value: &str) -> Result<SecretType, Box<dyn std::error::Error>> {
    match value {
        "api_key" => Ok(SecretType::ApiKey),
        "access_token" => Ok(SecretType::AccessToken),
        "refresh_token" => Ok(SecretType::RefreshToken),
        "password" => Ok(SecretType::Password),
        "passphrase" | "pass_phrase" => Ok(SecretType::Passphrase),
        "token" => Ok(SecretType::Token),
        "mnemonic" | "mnemonic_phrase" => Ok(SecretType::MnemonicPhrase),
        "ssh_key" => Ok(SecretType::SshKey),
        "private_key" => Ok(SecretType::PrivateKey),
        "certificate" | "cert" => Ok(SecretType::Certificate),
        "credential" => Ok(SecretType::Credential),
        "json_credential" => Ok(SecretType::JsonCredential),
        "generic" => Ok(SecretType::Generic),
        _ => Err(format!("unsupported secret type: {value}").into()),
    }
}

fn parse_delivery_mode(value: &str) -> Result<DeliveryMode, Box<dyn std::error::Error>> {
    match value {
        "brokered_http" => Ok(DeliveryMode::BrokeredHttp),
        "brokered_operation" => Ok(DeliveryMode::BrokeredOperation),
        "callback" => Ok(DeliveryMode::Callback),
        "pipe" => Ok(DeliveryMode::Pipe),
        "fd" => Ok(DeliveryMode::Fd),
        "temp_file" => Ok(DeliveryMode::TempFile),
        "child_env" => Ok(DeliveryMode::ChildEnv),
        "raw" => Ok(DeliveryMode::Raw),
        "terminal_print" => Ok(DeliveryMode::TerminalPrint),
        _ => Err(format!("unsupported delivery mode: {value}").into()),
    }
}

fn build_access_request(
    secret_ref: String,
    consumer: String,
    purpose: String,
    delivery_mode: &str,
    raw_export_requested: bool,
    provider_assurance: Option<String>,
    http_host: Option<String>,
    http_scheme: Option<String>,
    http_method: Option<String>,
    http_path: Option<String>,
    http_request_body_bytes: Option<u64>,
) -> Result<AccessRequest, Box<dyn std::error::Error>> {
    Ok(AccessRequest {
        secret_ref,
        consumer,
        purpose,
        delivery_mode: parse_delivery_mode(delivery_mode)?,
        provider_assurance: provider_assurance.unwrap_or_else(|| "A1".to_string()),
        raw_export_requested,
        http_host,
        http_scheme,
        http_method,
        http_path,
        http_request_body_bytes,
        os_uid: None,
        executable_path: None,
        executable_sha256: None,
        mfa_verified: false,
        broker_session_id: None,
        now: chrono::Utc::now(),
    })
}

fn provider_assurance(vault: &LocalVault) -> Result<String, Box<dyn std::error::Error>> {
    Ok(vault
        .status()?
        .provider_binding
        .get("assurance_level")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("A1")
        .to_string())
}

fn strip_separator(command: Vec<String>) -> Vec<String> {
    if command.first().is_some_and(|value| value == "--") {
        command.into_iter().skip(1).collect()
    } else {
        command
    }
}

fn parse_env_assignments(
    values: Vec<String>,
) -> Result<std::collections::BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let mut assignments = std::collections::BTreeMap::new();
    for value in values {
        let Some((key, value)) = value.split_once('=') else {
            return Err(format!("environment assignment must use NAME=value: {value}").into());
        };
        let key = key.trim();
        if key.is_empty() {
            return Err("environment assignment name must not be empty".into());
        }
        assignments.insert(key.to_string(), value.to_string());
    }
    Ok(assignments)
}

fn materialization_request(
    vault: &LocalVault,
    secret_ref: String,
    consumer: &str,
    purpose: &str,
    delivery_mode: DeliveryMode,
    mfa_verified: bool,
) -> Result<AccessRequest, Box<dyn std::error::Error>> {
    Ok(AccessRequest {
        secret_ref,
        consumer: consumer.to_string(),
        purpose: purpose.to_string(),
        delivery_mode,
        provider_assurance: provider_assurance(vault)?,
        raw_export_requested: false,
        http_host: None,
        http_scheme: None,
        http_method: None,
        http_path: None,
        http_request_body_bytes: None,
        os_uid: None,
        executable_path: None,
        executable_sha256: None,
        mfa_verified,
        broker_session_id: None,
        now: chrono::Utc::now(),
    })
}

fn exec_child(
    command: Vec<String>,
    child_env: std::collections::BTreeMap<String, String>,
    stdin_secret: Option<Vec<u8>>,
) -> Result<i32, Box<dyn std::error::Error>> {
    let mut child = ProcessCommand::new(&command[0]);
    child.args(&command[1..]).envs(child_env);
    if stdin_secret.is_some() {
        child.stdin(Stdio::piped());
    }
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = child.spawn()?;
    if let Some(secret) = stdin_secret {
        let mut stdin = child
            .stdin
            .take()
            .ok_or("failed to open child stdin for secret pipe")?;
        stdin.write_all(&secret)?;
    }
    let output = child.wait_with_output()?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(output.status.code().unwrap_or(1))
}

#[cfg(unix)]
fn make_fd_inheritable(file: &fs::File) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::fd::AsRawFd;
    let fd = file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_fd_inheritable(_file: &fs::File) -> Result<(), Box<dyn std::error::Error>> {
    Err("fd delivery is only supported on Unix".into())
}

fn secret_type_string(secret_type: SecretType) -> String {
    serde_json::to_value(secret_type)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn secret_record_summary(record: &hbse::records::SecretRecord) -> serde_json::Value {
    json!({
        "secret_ref": record.secret_ref,
        "secret_id": record.secret_id,
        "namespace_id": record.namespace_id,
        "latest_version": record.secret_version,
        "status": status_string(record.status),
        "secret_type": secret_type_string(record.secret_type),
        "created_at": record.created_at,
        "policy_id": record.policy_id,
        "policy_hash": record.policy_hash,
        "metadata_hash": record.metadata_hash,
        "provider_policy_hash": record.provider_policy_hash,
        "algorithm_id": record.algorithm_id,
        "wrap_algorithm_id": record.wrap_algorithm_id,
        "secret_aad_hash": record.secret_aad_hash,
        "dek_wrap_aad_hash": record.dek_wrap_aad_hash,
    })
}

fn print_vault_status(
    header: &hbse::store::VaultHeader,
    as_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let status = json!({
        "vault_id": header.vault_id,
        "namespace_id": header.namespace_id,
        "schema_version": header.schema_version,
        "created_at": header.created_at,
        "provider_id": header.provider_binding.get("provider_id").and_then(|value| value.as_str()),
        "assurance_level": header.provider_binding.get("assurance_level").and_then(|value| value.as_str()),
    });
    if as_json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("vault_id: {}", header.vault_id);
        println!("namespace_id: {}", header.namespace_id);
        println!(
            "provider_id: {}",
            status["provider_id"].as_str().unwrap_or("unknown")
        );
        println!(
            "assurance_level: {}",
            status["assurance_level"].as_str().unwrap_or("unknown")
        );
    }
    Ok(())
}

fn doctor_report(vault: &LocalVault) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut checks = serde_json::Map::new();
    checks.insert("schema".to_string(), json!("ok"));
    checks.insert(
        "vault_path".to_string(),
        json!(vault.store.path().to_string_lossy().to_string()),
    );
    checks.insert(
        "vault_file_exists".to_string(),
        json!(vault.store.path().exists()),
    );
    if let Ok(metadata) = fs::metadata(vault.store.path()) {
        checks.insert("vault_file_bytes".to_string(), json!(metadata.len()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            checks.insert(
                "vault_file_mode".to_string(),
                json!(format!("{:03o}", metadata.permissions().mode() & 0o777)),
            );
        }
    }
    let default_socket = default_runtime_socket_path();
    checks.insert(
        "default_broker_socket".to_string(),
        json!(default_socket.to_string_lossy().to_string()),
    );
    checks.insert(
        "default_broker_socket_exists".to_string(),
        json!(default_socket.exists()),
    );
    if let Ok(metadata) = fs::metadata(&default_socket) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            checks.insert(
                "default_broker_socket_mode".to_string(),
                json!(format!("{:03o}", metadata.permissions().mode() & 0o777)),
            );
        }
    }
    checks.insert(
        "default_broker_socket_status".to_string(),
        broker_socket_report(&default_socket),
    );
    let header = match vault.status() {
        Ok(header) => header,
        Err(hbse::vault::VaultError::Store(hbse::store::StoreError::VaultNotInitialized)) => {
            checks.insert("vault".to_string(), json!("not_initialized"));
            checks.insert(
                "provider_catalog".to_string(),
                json!(sanitized_provider_catalog("/dev/tpmrm0")),
            );
            return Ok(json!({"checks": checks}));
        }
        Err(err) => return Err(Box::new(err)),
    };
    checks.insert("vault".to_string(), json!("initialized"));
    checks.insert(
        "provider".to_string(),
        json!(header
            .provider_binding
            .get("provider_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")),
    );
    checks.insert(
        "provider_assurance".to_string(),
        json!(header
            .provider_binding
            .get("assurance_level")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")),
    );
    checks.insert(
        "store_integrity".to_string(),
        json!(vault.store.integrity_check()?),
    );
    checks.insert("secrets".to_string(), json!(vault.list_secrets()?.len()));
    checks.insert("policies".to_string(), json!(vault.list_policies()?.len()));
    let tickets = vault.list_tickets()?;
    checks.insert("tickets".to_string(), json!(tickets.len()));
    checks.insert(
        "active_tickets".to_string(),
        json!(tickets.iter().filter(|ticket| !ticket.revoked).count()),
    );
    checks.insert(
        "audit_events".to_string(),
        json!(vault.list_audit_events()?.len()),
    );
    let fingerprint_count = vault.store.count_redaction_fingerprints()?;
    checks.insert(
        "redaction_fingerprints".to_string(),
        json!(fingerprint_count),
    );
    checks.insert(
        "explicit_policy_configured".to_string(),
        json!(!vault.list_policies()?.is_empty()),
    );
    checks.insert(
        "audit_present".to_string(),
        json!(!vault.list_audit_events()?.is_empty()),
    );
    checks.insert("redaction_ready".to_string(), json!(fingerprint_count > 0));
    let tpm_device = header
        .provider_binding
        .get("device_path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("/dev/tpmrm0");
    checks.insert(
        "provider_catalog".to_string(),
        json!(sanitized_provider_catalog(tpm_device)),
    );
    Ok(json!({"checks": checks}))
}

fn setup_report(
    vault: &LocalVault,
    vault_path: &std::path::Path,
    tpm_device: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let providers = local_provider_catalog(tpm_device);
    let direct_tpm_available = provider_available(&providers, TPM2_ESAPI_PROVIDER_ID);
    let tools_tpm_available = provider_available(&providers, TPM2_PROVIDER_ID);
    let system_fingerprint_available =
        provider_available(&providers, SYSTEM_FINGERPRINT_PROVIDER_ID);
    let recommended_provider = if direct_tpm_available {
        "tpm2-direct"
    } else if tools_tpm_available {
        "tpm2"
    } else if system_fingerprint_available {
        "system-fingerprint"
    } else {
        "passphrase"
    };
    let vault_status = match vault.status() {
        Ok(header) => json!({
            "initialized": true,
            "vault_id": header.vault_id,
            "namespace_id": header.namespace_id,
            "provider_id": header.provider_binding.get("provider_id").and_then(serde_json::Value::as_str),
            "assurance_level": header.provider_binding.get("assurance_level").and_then(serde_json::Value::as_str),
        }),
        Err(hbse::vault::VaultError::Store(hbse::store::StoreError::VaultNotInitialized)) => {
            json!({"initialized": false})
        }
        Err(err) => return Err(Box::new(err)),
    };
    let init_command = match recommended_provider {
        "passphrase" => format!(
            "hbse --vault {} vault init --provider passphrase",
            shell_quote_path(vault_path)
        ),
        "system-fingerprint" => format!(
            "hbse --vault {} vault init --provider system-fingerprint",
            shell_quote_path(vault_path)
        ),
        provider => format!(
            "hbse --vault {} vault init --provider {provider} --tpm-device {}",
            shell_quote_path(vault_path),
            shell_quote(tpm_device)
        ),
    };
    let socket_path = default_runtime_socket_path();
    let broker_status = broker_socket_report(&socket_path);
    let install_service_command = format!(
        "hbse --vault {} broker install-service --scope user --enable --start",
        shell_quote_path(vault_path)
    );
    Ok(json!({
        "vault_path": vault_path.to_string_lossy().to_string(),
        "vault": vault_status,
        "recommended_provider": recommended_provider,
        "provider_catalog": sanitized_provider_catalog(tpm_device),
        "broker": {
            "default_socket": socket_path.to_string_lossy().to_string(),
            "default_socket_exists": socket_path.exists(),
            "default_socket_reachable": broker_status.get("reachable").and_then(serde_json::Value::as_bool).unwrap_or(false),
            "default_socket_detail": broker_status.get("detail").and_then(serde_json::Value::as_str).unwrap_or("unknown"),
            "user_service_file_exists": user_service_file_exists(),
        },
        "commands": {
            "initialize_vault": init_command,
            "install_user_broker_service": install_service_command,
            "check_readiness": format!("hbse --vault {} readiness check --verify-audit", shell_quote_path(vault_path)),
            "deep_diagnostics": format!("hbse --vault {} doctor", shell_quote_path(vault_path)),
        },
    }))
}

fn broker_socket_report(socket: &std::path::Path) -> serde_json::Value {
    let mut report = serde_json::Map::new();
    report.insert(
        "path".to_string(),
        json!(socket.to_string_lossy().to_string()),
    );
    report.insert("exists".to_string(), json!(socket.exists()));
    if let Ok(metadata) = fs::metadata(socket) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            report.insert(
                "mode".to_string(),
                json!(format!("{:03o}", metadata.permissions().mode() & 0o777)),
            );
        }
    }
    match broker_daemon::request(socket, &json!({"command": "status"})) {
        Ok(status) => {
            report.insert("reachable".to_string(), json!(true));
            report.insert("detail".to_string(), json!("broker reachable"));
            report.insert(
                "unlocked".to_string(),
                json!(status
                    .get("unlocked")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)),
            );
            report.insert(
                "mfa_verified".to_string(),
                json!(status
                    .get("mfa_verified")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)),
            );
        }
        Err(err) => {
            report.insert("reachable".to_string(), json!(false));
            report.insert("detail".to_string(), json!(err.to_string()));
        }
    }
    serde_json::Value::Object(report)
}

fn cleanup_broker_socket(
    socket: &std::path::Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if !socket.exists() {
        return Ok(json!({
            "socket": socket.to_string_lossy().to_string(),
            "removed": false,
            "detail": "broker socket does not exist",
        }));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let metadata = fs::metadata(socket)?;
        if !metadata.file_type().is_socket() {
            return Ok(json!({
                "socket": socket.to_string_lossy().to_string(),
                "removed": false,
                "detail": "path exists but is not a Unix socket; refusing to remove it",
            }));
        }
    }
    if broker_daemon::request(socket, &json!({"command": "status"})).is_ok() {
        return Ok(json!({
            "socket": socket.to_string_lossy().to_string(),
            "removed": false,
            "detail": "broker is reachable; socket was not removed",
        }));
    }
    fs::remove_file(socket)?;
    Ok(json!({
        "socket": socket.to_string_lossy().to_string(),
        "removed": true,
        "detail": "removed stale broker socket",
    }))
}

fn provider_available(
    providers: &[hbse::provider_catalog::ProviderCatalogEntry],
    id: &str,
) -> bool {
    providers
        .iter()
        .any(|provider| provider.provider_id == id && provider.available)
}

fn sanitized_provider_catalog(tpm_device: &str) -> Vec<serde_json::Value> {
    local_provider_catalog(tpm_device)
        .into_iter()
        .map(|provider| {
            json!({
                "provider_id": provider.provider_id,
                "name": provider.name,
                "assurance_level": provider.assurance_level,
                "hardware_backed": provider.hardware_backed,
                "vault_binding_supported": provider.vault_binding_supported,
                "available": provider.available,
                "detail": provider.detail,
                "warning": provider.warning,
            })
        })
        .collect()
}

fn print_setup_report(report: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "vault_path: {}",
        printable_json_value(&report["vault_path"])
    );
    println!(
        "vault_initialized: {}",
        report["vault"]["initialized"].as_bool().unwrap_or(false)
    );
    println!(
        "recommended_provider: {}",
        printable_json_value(&report["recommended_provider"])
    );
    println!("providers:");
    if let Some(providers) = report["provider_catalog"].as_array() {
        for provider in providers {
            println!(
                "  {} {} {}",
                printable_json_value(&provider["provider_id"]),
                if provider["available"].as_bool().unwrap_or(false) {
                    "available"
                } else {
                    "unavailable"
                },
                printable_json_value(&provider["detail"]),
            );
        }
    }
    println!("commands:");
    if let Some(commands) = report["commands"].as_object() {
        for (name, command) in commands {
            println!("  {name}: {}", printable_json_value(command));
        }
    }
    Ok(())
}

fn readiness_report(
    vault: &LocalVault,
    target: &str,
    release_dir: PathBuf,
    verify_audit: bool,
    passphrase: Option<String>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut items = Vec::new();
    let header = match vault.status() {
        Ok(header) => header,
        Err(hbse::vault::VaultError::Store(hbse::store::StoreError::VaultNotInitialized)) => {
            items.push(readiness_item("Vault", "fail", "vault is not initialized"));
            return Ok(json!({"target": target, "passed": false, "items": items}));
        }
        Err(err) => return Err(Box::new(err)),
    };
    items.push(readiness_item("Vault", "pass", "vault initialized"));
    let provider_level = header
        .provider_binding
        .get("assurance_level")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("A0");
    if provider_level >= "A1" {
        items.push(readiness_item(
            "Providers",
            "pass",
            format!("provider assurance visible: {provider_level}"),
        ));
    } else {
        items.push(readiness_item(
            "Providers",
            "fail",
            "provider assurance below A1",
        ));
    }
    if vault.list_policies()?.is_empty() {
        items.push(readiness_item(
            "Policy",
            "fail",
            "no explicit policies configured",
        ));
    } else {
        items.push(readiness_item(
            "Policy",
            "pass",
            "at least one explicit policy exists",
        ));
    }
    if vault.list_audit_events()?.is_empty() {
        items.push(readiness_item("Audit", "fail", "no audit events recorded"));
    } else {
        items.push(readiness_item("Audit", "pass", "audit events exist"));
    }
    if vault.store.count_redaction_fingerprints()? > 0 {
        items.push(readiness_item(
            "Redaction",
            "pass",
            "secret fingerprints exist",
        ));
    } else {
        items.push(readiness_item(
            "Redaction",
            "warn",
            "no active secret fingerprints yet",
        ));
    }
    if verify_audit {
        let passphrase = unlock_passphrase(vault, passphrase)?;
        match vault.verify_audit(&passphrase) {
            Ok(()) => items.push(readiness_item("Audit", "pass", "audit chain verifies")),
            Err(err) => items.push(readiness_item(
                "Audit",
                "fail",
                format!("audit chain verification failed: {err}"),
            )),
        }
    } else {
        items.push(readiness_item(
            "Audit",
            "warn",
            "audit MAC not verified; pass --verify-audit",
        ));
    }
    add_release_readiness_items(&mut items, &release_dir, target);
    if matches!(target, "A4" | "A5") {
        items.push(readiness_item(
            "Review",
            "fail",
            "independent security review evidence is required and cannot be generated automatically",
        ));
        items.push(readiness_item(
            "Providers",
            "fail",
            "real hardware copied-vault tests require external TPM/provider evidence",
        ));
    }
    let passed = items
        .iter()
        .all(|item| item["status"].as_str() != Some("fail"));
    Ok(json!({"target": target, "passed": passed, "items": items}))
}

fn add_release_readiness_items(
    items: &mut Vec<serde_json::Value>,
    release_dir: &std::path::Path,
    target: &str,
) {
    for (area, path) in [
        ("SBOM", release_dir.join("sbom.json")),
        ("Provenance", release_dir.join("provenance.json")),
        ("Signature", release_dir.join("artifact.sig")),
        ("Checklist", release_dir.join("production_checklist.json")),
    ] {
        if path.exists() {
            items.push(readiness_item(
                area,
                "pass",
                format!("{} exists", path.display()),
            ));
        } else {
            let status = if matches!(target, "A4" | "A5") {
                "fail"
            } else {
                "warn"
            };
            items.push(readiness_item(
                area,
                status,
                format!("{} missing", path.display()),
            ));
        }
    }
}

fn readiness_item(
    area: impl Into<String>,
    status: impl Into<String>,
    detail: impl Into<String>,
) -> serde_json::Value {
    json!({
        "area": area.into(),
        "status": status.into(),
        "detail": detail.into(),
    })
}

fn print_checks(
    report: &serde_json::Value,
    as_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }
    if let Some(checks) = report.get("checks").and_then(serde_json::Value::as_object) {
        for (key, value) in checks {
            if key == "provider_catalog" {
                println!("{key}:");
                if let Some(providers) = value.as_array() {
                    for provider in providers {
                        println!(
                            "  {} {} {}",
                            printable_json_value(&provider["provider_id"]),
                            if provider["available"].as_bool().unwrap_or(false) {
                                "available"
                            } else {
                                "unavailable"
                            },
                            printable_json_value(&provider["detail"]),
                        );
                    }
                }
                continue;
            }
            println!("{key}: {}", printable_json_value(value));
        }
    }
    Ok(())
}

fn printable_json_value(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn default_store_path() -> PathBuf {
    if let Ok(path) = env::var("HBSE_VAULT_PATH") {
        return PathBuf::from(path);
    }
    match env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(".local/share/hbse/vault.db"),
        Err(_) => PathBuf::from("vault.db"),
    }
}

fn default_runtime_socket_path() -> PathBuf {
    env::var_os("HBSE_BROKER_SOCKET")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .map(|path| path.join("hbse/broker.sock"))
        })
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share/hbse/broker.sock")
        })
}

fn user_service_file_exists() -> bool {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        })
        .join("systemd/user/hbse-broker.service")
        .exists()
}

fn shell_quote_path(path: &std::path::Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
