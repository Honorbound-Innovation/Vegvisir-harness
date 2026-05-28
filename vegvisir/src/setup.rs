use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;

use crate::{
    core::{ConfigStore, ProviderRegistry},
    memory::default_vegvisir_data_root,
    provider::hbse_default_or_configured_socket,
};

const PROVIDER_CHOICES: &[ProviderChoice] = &[
    ProviderChoice {
        label: "OpenAI",
        provider_id: "openai",
        hbse_provider: "openai-hbse",
        default_model: "gpt-5.2",
    },
    ProviderChoice {
        label: "Anthropic Claude",
        provider_id: "anthropic",
        hbse_provider: "anthropic-hbse",
        default_model: "claude-sonnet-4.5",
    },
    ProviderChoice {
        label: "Google Gemini",
        provider_id: "google",
        hbse_provider: "google-hbse",
        default_model: "gemini-2.5-pro",
    },
    ProviderChoice {
        label: "OpenRouter",
        provider_id: "openrouter",
        hbse_provider: "openrouter-hbse",
        default_model: "openrouter:auto",
    },
    ProviderChoice {
        label: "Groq",
        provider_id: "groq",
        hbse_provider: "groq-hbse",
        default_model: "llama-3.3-70b-versatile",
    },
    ProviderChoice {
        label: "xAI",
        provider_id: "xai",
        hbse_provider: "xai-hbse",
        default_model: "grok-4",
    },
];

#[derive(Clone, Copy)]
struct ProviderChoice {
    label: &'static str,
    provider_id: &'static str,
    hbse_provider: &'static str,
    default_model: &'static str,
}

#[derive(Debug, Clone)]
pub struct SetupOptions {
    pub data_root: PathBuf,
    pub workspace: PathBuf,
    pub non_interactive: bool,
    pub force: bool,
    pub provider: Option<String>,
    pub skip_hbse: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupSummary {
    pub data_root: PathBuf,
    pub workspace: PathBuf,
    pub config_path: PathBuf,
    pub current_provider: String,
    pub current_model: String,
    pub hbse_socket: PathBuf,
    pub hbse_socket_exists: bool,
    pub hbse_onboarding_command: Option<String>,
    pub next_steps: Vec<String>,
}

pub fn run_setup(options: SetupOptions) -> anyhow::Result<SetupSummary> {
    if options.non_interactive {
        run_setup_non_interactive(options)
    } else {
        run_setup_interactive(options)
    }
}

fn run_setup_non_interactive(options: SetupOptions) -> anyhow::Result<SetupSummary> {
    let choice = choice_from_provider(options.provider.as_deref()).unwrap_or(PROVIDER_CHOICES[0]);
    apply_setup(options, choice)
}

fn run_setup_interactive(options: SetupOptions) -> anyhow::Result<SetupSummary> {
    print_intro(&options)?;
    let choice = select_provider(options.provider.as_deref())?;
    let summary = apply_setup(options, choice)?;
    print_summary(&summary)?;
    Ok(summary)
}

fn apply_setup(options: SetupOptions, choice: ProviderChoice) -> anyhow::Result<SetupSummary> {
    fs::create_dir_all(&options.data_root)
        .with_context(|| format!("failed to create {}", options.data_root.display()))?;
    fs::create_dir_all(options.data_root.join("sessions"))?;
    fs::create_dir_all(options.data_root.join("agents"))?;
    fs::create_dir_all(options.data_root.join("skills"))?;
    fs::create_dir_all(options.data_root.join("traces"))?;

    let config_path = options.data_root.join("config.json");
    let store = ConfigStore::new(&config_path);
    let mut config = store.load().unwrap_or_default();

    if options.force || !config.contains_key("current_provider") {
        config.insert(
            "current_provider".to_string(),
            Value::String(choice.hbse_provider.to_string()),
        );
    }
    if options.force || !config.contains_key("current_model") {
        config.insert(
            "current_model".to_string(),
            Value::String(choice.default_model.to_string()),
        );
    }
    config
        .entry("setup_completed".to_string())
        .or_insert_with(|| Value::Bool(true));
    config.insert(
        "setup_workspace".to_string(),
        Value::String(options.workspace.display().to_string()),
    );
    config.insert(
        "auth_mode_hint".to_string(),
        Value::String("hbse".to_string()),
    );
    store.save(&config)?;

    let provider_registry = ProviderRegistry::default_catalog()?;
    let hbse_socket = provider_registry
        .get(choice.hbse_provider)
        .map(hbse_default_or_configured_socket)
        .unwrap_or_else(default_hbse_socket_path);
    let hbse_socket_exists = hbse_socket.exists();

    let hbse_onboarding_command = (!options.skip_hbse)
        .then(|| format!("scripts/hbse-provider-onboard.sh {}", choice.provider_id));
    let mut next_steps = Vec::new();
    if !options.skip_hbse {
        next_steps.push(format!(
            "Register your provider credential in HBSE: {}",
            hbse_onboarding_command.as_deref().unwrap_or("")
        ));
    }
    if !hbse_socket_exists {
        next_steps.push(format!(
            "Start or configure the HBSE broker so its socket exists at {} (or set HBSE_BROKER_SOCKET).",
            hbse_socket.display()
        ));
    }
    next_steps.push("Launch Vegvisir with `vegvisir` and send a short test prompt.".to_string());

    Ok(SetupSummary {
        data_root: options.data_root,
        workspace: options.workspace,
        config_path,
        current_provider: config
            .get("current_provider")
            .and_then(Value::as_str)
            .unwrap_or(choice.hbse_provider)
            .to_string(),
        current_model: config
            .get("current_model")
            .and_then(Value::as_str)
            .unwrap_or(choice.default_model)
            .to_string(),
        hbse_socket,
        hbse_socket_exists,
        hbse_onboarding_command,
        next_steps,
    })
}

fn print_intro(options: &SetupOptions) -> anyhow::Result<()> {
    println!("Vegvisir first-time setup");
    println!("────────────────────────");
    println!(
        "This creates Vegvisir's local data directories and selects an HBSE-routed model provider."
    );
    println!("Vegvisir will not ask for or store plaintext provider keys here.");
    println!();
    println!("Data root:  {}", options.data_root.display());
    println!("Workspace:  {}", options.workspace.display());
    println!();
    Ok(())
}

fn select_provider(preselected: Option<&str>) -> anyhow::Result<ProviderChoice> {
    if let Some(choice) = choice_from_provider(preselected) {
        println!(
            "Selected provider: {} ({})",
            choice.label, choice.hbse_provider
        );
        return Ok(choice);
    }
    if !io::stdin().is_terminal() {
        return Ok(PROVIDER_CHOICES[0]);
    }
    println!("Choose a default provider:");
    for (index, choice) in PROVIDER_CHOICES.iter().enumerate() {
        println!("  {}) {} via HBSE", index + 1, choice.label);
    }
    print!("Provider [1]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let selected = input
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|n| PROVIDER_CHOICES.get(n.saturating_sub(1)))
        .copied()
        .unwrap_or(PROVIDER_CHOICES[0]);
    Ok(selected)
}

fn print_summary(summary: &SetupSummary) -> anyhow::Result<()> {
    println!();
    println!("Setup written:");
    println!("  config:   {}", summary.config_path.display());
    println!("  provider: {}", summary.current_provider);
    println!("  model:    {}", summary.current_model);
    println!(
        "  HBSE:     {} (exists={})",
        summary.hbse_socket.display(),
        summary.hbse_socket_exists
    );
    println!();
    println!("Next steps:");
    for (index, step) in summary.next_steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }
    println!();
    println!("Tip: use `/hbse status` inside Vegvisir to inspect HBSE readiness.");
    Ok(())
}

fn choice_from_provider(provider: Option<&str>) -> Option<ProviderChoice> {
    let provider = provider?.trim().trim_end_matches("-hbse");
    PROVIDER_CHOICES
        .iter()
        .find(|choice| choice.provider_id.eq_ignore_ascii_case(provider))
        .copied()
}

fn default_hbse_socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HBSE_BROKER_SOCKET") {
        return PathBuf::from(path);
    }
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("hbse").join("broker.sock");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/hbse/broker.sock")
}

pub fn default_setup_options(workspace: impl AsRef<Path>) -> SetupOptions {
    SetupOptions {
        data_root: default_vegvisir_data_root(),
        workspace: workspace.as_ref().to_path_buf(),
        non_interactive: false,
        force: false,
        provider: None,
        skip_hbse: false,
    }
}

pub fn setup_status(data_root: impl AsRef<Path>) -> anyhow::Result<SetupSummary> {
    let data_root = data_root.as_ref().to_path_buf();
    let config_path = data_root.join("config.json");
    let config = ConfigStore::new(&config_path).load().unwrap_or_default();
    let provider = config
        .get("current_provider")
        .and_then(Value::as_str)
        .unwrap_or("demo")
        .to_string();
    let model = config
        .get("current_model")
        .and_then(Value::as_str)
        .unwrap_or("demo-local")
        .to_string();
    let provider_registry = ProviderRegistry::default_catalog()?;
    let hbse_socket = provider_registry
        .get(&provider)
        .map(hbse_default_or_configured_socket)
        .unwrap_or_else(default_hbse_socket_path);
    Ok(SetupSummary {
        data_root,
        workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        config_path,
        current_provider: provider,
        current_model: model,
        hbse_socket: hbse_socket.clone(),
        hbse_socket_exists: hbse_socket.exists(),
        hbse_onboarding_command: None,
        next_steps: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_interactive_setup_writes_hbse_defaults() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let data_root = tmp.path().join("data");
        let workspace = tmp.path().join("workspace");
        let summary = run_setup(SetupOptions {
            data_root: data_root.clone(),
            workspace: workspace.clone(),
            non_interactive: true,
            force: false,
            provider: Some("anthropic".to_string()),
            skip_hbse: false,
        })?;

        assert_eq!(summary.current_provider, "anthropic-hbse");
        assert_eq!(summary.current_model, "claude-sonnet-4.5");
        assert!(summary.config_path.exists());
        assert!(data_root.join("sessions").is_dir());
        assert!(
            summary
                .next_steps
                .iter()
                .any(|step| step.contains("hbse-provider-onboard.sh anthropic"))
        );

        let config: Value = serde_json::from_str(&std::fs::read_to_string(summary.config_path)?)?;
        assert_eq!(config["auth_mode_hint"], "hbse");
        assert_eq!(config["setup_workspace"], workspace.display().to_string());
        Ok(())
    }

    #[test]
    fn setup_status_reports_existing_config() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let data_root = tmp.path().join("data");
        run_setup(SetupOptions {
            data_root: data_root.clone(),
            workspace: tmp.path().join("workspace"),
            non_interactive: true,
            force: false,
            provider: Some("openai".to_string()),
            skip_hbse: true,
        })?;

        let summary = setup_status(&data_root)?;
        assert_eq!(summary.current_provider, "openai-hbse");
        assert_eq!(summary.current_model, "gpt-5.2");
        assert!(summary.next_steps.is_empty());
        Ok(())
    }
}
