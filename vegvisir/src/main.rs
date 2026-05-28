use std::path::PathBuf;

use clap::{Parser, Subcommand};
use vegvisir_rust::{
    AgentHarness, AgentTask, ScriptedModel,
    app::{TuiApplication, run_tui_with_dangerous_bypass, workspace_project_id},
    bridge::{BridgeOptions, run_app_server},
    compat_server::{CompatServerOptions, run_openai_compat_server},
    evals::{format_eval_results, run_builtin_evals, run_eval_file},
    memory::{VegvisirCms, VegvisirCmsConfig, default_vegvisir_data_root},
    setup::{SetupOptions, run_setup, setup_status},
};

#[derive(Parser)]
#[command(name = "vegvisir")]
struct Cli {
    #[arg(short, long)]
    prompt: Option<String>,
    #[arg(long, default_value_os_t = current_workspace())]
    workspace: PathBuf,
    #[arg(long, default_value_t = 4)]
    max_steps: usize,
    #[arg(long, global = true)]
    provider: Option<String>,
    #[arg(long, global = true)]
    model: Option<String>,
    #[arg(long, global = true)]
    agent: Option<String>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    scripted: bool,
    #[arg(long, global = true)]
    dangerously_bypass_approvals_and_sandbox: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Tui,
    Run {
        goal: String,
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
        #[arg(long, default_value_t = 4)]
        max_steps: usize,
    },
    Remember {
        title: String,
        content: String,
        #[arg(long, default_value = "note")]
        memory_type: String,
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
    },
    Recall {
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
    },
    Context {
        message: String,
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
    },
    ModelRequest {
        message: String,
        #[arg(long, default_value = "local")]
        provider: String,
        #[arg(long, default_value = "unspecified")]
        model: String,
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
    },
    Eval {
        #[arg(default_value = "all")]
        scope: String,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    Verify {
        #[arg(default_value = "all")]
        scope: String,
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
    },
    AppServer {
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
    },
    OpenAiCompatServer {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 11435)]
        port: u16,
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
    },
    /// Run first-time setup or inspect setup status.
    Setup {
        /// Print current setup status instead of writing setup config.
        #[arg(long)]
        status: bool,
        /// Vegvisir data root. Defaults to the platform Vegvisir data directory.
        #[arg(long)]
        data_root: Option<PathBuf>,
        /// Workspace to record as the initial/default workspace.
        #[arg(long, default_value_os_t = current_workspace())]
        workspace: PathBuf,
        /// Use defaults and avoid prompts.
        #[arg(long)]
        non_interactive: bool,
        /// Overwrite current_provider/current_model even if already configured.
        #[arg(long)]
        force: bool,
        /// Do not include HBSE onboarding instructions in next steps.
        #[arg(long)]
        skip_hbse: bool,
    },
    /// Run the integrated Skiller component. Use `vegvisir skiller -- <args>`.
    Skiller {
        #[arg(last = true)]
        args: Vec<std::ffi::OsString>,
    },
}

fn current_workspace() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(prompt) = cli.prompt {
        run_headless(
            prompt,
            cli.workspace,
            cli.max_steps,
            cli.provider,
            cli.model,
            cli.agent,
            cli.json,
            cli.scripted,
            cli.dangerously_bypass_approvals_and_sandbox,
        )
    } else {
        match cli.command {
            Some(Command::Run {
                goal,
                workspace,
                max_steps,
            }) => run_headless(
                goal,
                workspace,
                max_steps,
                cli.provider,
                cli.model,
                cli.agent,
                cli.json,
                cli.scripted,
                cli.dangerously_bypass_approvals_and_sandbox,
            ),
            Some(Command::Remember {
                title,
                content,
                memory_type,
                workspace,
            }) => run_remember(workspace, memory_type, title, content),
            Some(Command::Recall {
                query,
                limit,
                workspace,
            }) => run_recall(workspace, query, limit),
            Some(Command::Context { message, workspace }) => run_context(workspace, message),
            Some(Command::ModelRequest {
                message,
                provider,
                model,
                workspace,
            }) => run_model_request(workspace, message, provider, model),
            Some(Command::Eval { scope, file }) => run_eval(scope, file),
            Some(Command::Verify { scope, workspace }) => run_verify(
                workspace,
                scope,
                cli.dangerously_bypass_approvals_and_sandbox,
            ),
            Some(Command::AppServer { workspace }) => run_app_server(BridgeOptions {
                workspace,
                data_root: None,
                provider: cli.provider,
                model: cli.model,
                agent: cli.agent,
                dangerously_bypass_approvals_and_sandbox: cli
                    .dangerously_bypass_approvals_and_sandbox,
            }),
            Some(Command::OpenAiCompatServer {
                host,
                port,
                workspace,
            }) => run_openai_compat_server(CompatServerOptions {
                host,
                port,
                workspace,
                provider: cli.provider,
                model: cli.model,
                agent: cli.agent,
                dangerously_bypass_approvals_and_sandbox: cli
                    .dangerously_bypass_approvals_and_sandbox,
            }),
            Some(Command::Setup {
                status,
                data_root,
                workspace,
                non_interactive,
                force,
                skip_hbse,
            }) => run_setup_command(
                status,
                data_root,
                workspace,
                non_interactive,
                force,
                skip_hbse,
                cli.provider,
                cli.json,
            ),
            Some(Command::Skiller { args }) => run_skiller(args),
            Some(Command::Tui) | None => {
                run_tui_with_dangerous_bypass(cli.dangerously_bypass_approvals_and_sandbox)
            }
        }
    }
}

fn run_skiller(args: Vec<std::ffi::OsString>) -> anyhow::Result<()> {
    let argv = std::iter::once(std::ffi::OsString::from("skiller")).chain(args);
    skiller::run_cli_from(argv)
}

fn run_setup_command(
    status: bool,
    data_root: Option<PathBuf>,
    workspace: PathBuf,
    non_interactive: bool,
    force: bool,
    skip_hbse: bool,
    provider: Option<String>,
    json_output: bool,
) -> anyhow::Result<()> {
    let data_root = data_root.unwrap_or_else(default_vegvisir_data_root);
    let summary = if status {
        setup_status(&data_root)?
    } else {
        run_setup(SetupOptions {
            data_root,
            workspace,
            non_interactive: non_interactive || json_output,
            force,
            provider,
            skip_hbse,
        })?
    };
    if json_output {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }
    if status {
        println!("Vegvisir setup status");
        println!("────────────────────");
        println!("data root: {}", summary.data_root.display());
        println!("config:    {}", summary.config_path.display());
        println!("provider:  {}", summary.current_provider);
        println!("model:     {}", summary.current_model);
        println!(
            "HBSE:      {} (exists={})",
            summary.hbse_socket.display(),
            summary.hbse_socket_exists
        );
        if !summary.hbse_socket_exists {
            println!();
            println!("Next: start/configure HBSE, or set HBSE_BROKER_SOCKET.");
        }
    }
    Ok(())
}

fn run_verify(
    workspace: PathBuf,
    scope: String,
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<()> {
    let mut app = TuiApplication::new_with_dangerous_bypass(
        workspace,
        dangerously_bypass_approvals_and_sandbox,
    )?;
    let output = app
        .execute_command(&format!("/verify {scope}"))?
        .unwrap_or_else(|| "No verification output.".to_string());
    println!("{output}");
    Ok(())
}

fn run_eval(scope: String, file: Option<PathBuf>) -> anyhow::Result<()> {
    let results = if let Some(file) = file {
        run_eval_file(file)?
    } else {
        run_builtin_evals(&scope)?
    };
    println!("{}", format_eval_results(&results));
    Ok(())
}

fn run_headless(
    prompt: String,
    workspace: PathBuf,
    max_steps: usize,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    json_output: bool,
    scripted: bool,
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<()> {
    if !scripted {
        return run_headless_provider(
            prompt,
            workspace,
            provider,
            model,
            agent,
            json_output,
            dangerously_bypass_approvals_and_sandbox,
        );
    }
    let model = ScriptedModel::default();
    let mut harness = if dangerously_bypass_approvals_and_sandbox {
        AgentHarness::with_dangerous_bypass(model, &workspace)?
    } else {
        AgentHarness::default(model, &workspace)?
    };
    let mut task = AgentTask::new(prompt, workspace);
    task.max_steps = max_steps;
    let result = harness.run(task)?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": result.status,
                "answer": result.final_answer,
                "steps": result.steps,
                "run_id": result.run_id,
                "checkpoint": result.checkpoint.as_ref().map(|path| path.display().to_string()),
                "snapshot": result.snapshot.as_ref().map(|path| path.display().to_string()),
                "mode": "scripted_harness",
            }))?
        );
        return Ok(());
    }
    println!(
        "{}: {}",
        result.status,
        result.final_answer.unwrap_or_default()
    );
    if let Some(checkpoint) = result.checkpoint {
        println!("checkpoint: {}", checkpoint.display());
    }
    Ok(())
}

fn run_headless_provider(
    prompt: String,
    workspace: PathBuf,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    json_output: bool,
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<()> {
    let mut app = TuiApplication::new_with_dangerous_bypass(
        &workspace,
        dangerously_bypass_approvals_and_sandbox,
    )?;
    if let Some(provider) = provider {
        apply_cli_command(&mut app, &format!("/provider {provider}"), "provider")?;
    }
    if let Some(model) = model {
        apply_cli_command(&mut app, &format!("/model {model}"), "model")?;
    }
    if let Some(agent) = agent {
        apply_cli_command(&mut app, &format!("/agent use {agent}"), "agent")?;
    }
    let response = app.send_headless(&prompt)?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "completed",
                "answer": response,
                "workspace": app.cwd.display().to_string(),
                "session_id": app.session.session_id,
                "provider": app.session.current_provider,
                "model": app.session.current_model,
                "agent": app.session.active_agent_id,
                "messages": app.session.messages.len(),
                "tokens_used": app.session.tokens_used,
                "latency_ms": app.session.last_latency_ms,
                "prompt_cache_key": app.session.last_prompt_cache_key,
                "mode": "provider_runtime",
            }))?
        );
    } else {
        println!("{response}");
    }
    Ok(())
}

fn apply_cli_command(app: &mut TuiApplication, command: &str, label: &str) -> anyhow::Result<()> {
    let output = app.execute_command(command)?.unwrap_or_default();
    if output.starts_with("Unknown ")
        || output.starts_with("Provider ")
        || output.starts_with("Model ")
        || output.contains(" is not available")
        || output.contains("Unknown provider")
        || output.contains("Unknown model")
        || output.contains("Unknown agent")
    {
        anyhow::bail!("{label} selection failed: {output}");
    }
    Ok(())
}

fn open_cms(workspace: PathBuf) -> anyhow::Result<VegvisirCms> {
    VegvisirCms::open(VegvisirCmsConfig {
        db_path: default_vegvisir_data_root().join("cms-v2.sqlite3"),
        user_id: "local-user".to_string(),
        project_id: Some(workspace_project_id(&workspace)),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })
}

fn run_remember(
    workspace: PathBuf,
    memory_type: String,
    title: String,
    content: String,
) -> anyhow::Result<()> {
    let mut cms = open_cms(workspace)?;
    let result = cms.remember(memory_type, title, content)?;
    println!("remembered: {}", result.memory_id.0);
    Ok(())
}

fn run_recall(workspace: PathBuf, query: String, limit: usize) -> anyhow::Result<()> {
    let mut cms = open_cms(workspace)?;
    let bundle = cms.retrieve(query, limit)?;
    if bundle.results.is_empty() {
        println!("No CMS memories matched.");
        return Ok(());
    }
    for result in bundle.results {
        println!(
            "{} [{}]\n{}",
            result.memory.title, result.memory.id.0, result.memory.summary
        );
    }
    Ok(())
}

fn run_context(workspace: PathBuf, message: String) -> anyhow::Result<()> {
    let mut cms = open_cms(workspace)?;
    let prepared = cms.prepare_context(message)?;
    println!("{}", prepared.packed_text);
    Ok(())
}

fn run_model_request(
    workspace: PathBuf,
    message: String,
    provider: String,
    model: String,
) -> anyhow::Result<()> {
    let mut cms = open_cms(workspace)?;
    let envelope = cms.prepare_cached_prompt(message, provider, model)?;
    println!("{}", serde_json::to_string_pretty(&envelope)?);
    Ok(())
}
