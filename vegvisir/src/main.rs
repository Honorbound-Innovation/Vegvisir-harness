use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use vegvisir_rust::{
    AgentHarness, AgentTask, ScriptedModel,
    app::{TuiApplication, run_tui_with_dangerous_bypass, workspace_project_id},
    bridge::{BridgeOptions, run_app_server},
    compat_server::{CompatServerOptions, run_openai_compat_server},
    evals::{format_eval_results, run_builtin_evals, run_eval_file},
    memory::{VegvisirCms, VegvisirCmsConfig, default_vegvisir_data_root},
    run_artifacts::{
        RunArtifactManager, RunManifest, RunStatus, RunVerificationCheck, RunVerificationEvidence,
        RunVerificationSource,
    },
    setup::{SetupOptions, run_setup, setup_status},
};

#[derive(Parser)]
#[command(name = "vegvisir", bin_name = "vegvisir")]
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
    artifacts: bool,
    #[arg(long, global = true)]
    artifact_dir: Option<PathBuf>,
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
        #[arg(long)]
        workspace: Option<PathBuf>,
        #[arg(long, default_value_t = 4)]
        max_steps: usize,
    },
    Remember {
        title: String,
        content: String,
        #[arg(long, default_value = "note")]
        memory_type: String,
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    Recall {
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    Context {
        message: String,
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    ModelRequest {
        message: String,
        #[arg(long, default_value = "local")]
        provider: String,
        #[arg(long, default_value = "unspecified")]
        model: String,
        #[arg(long)]
        workspace: Option<PathBuf>,
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
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    /// Launch the Vegvisir Desktop app.
    Desktop {
        /// Path to a packaged Vegvisir Desktop executable/AppImage.
        #[arg(long)]
        binary: Option<PathBuf>,
        /// Force launching the source checkout with `npm run dev`.
        #[arg(long)]
        dev: bool,
    },
    AppServer {
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    OpenAiCompatServer {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 11435)]
        port: u16,
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    /// Run first-time setup or inspect setup status.
    Setup {
        /// Print current setup status instead of writing setup config.
        #[arg(long)]
        status: bool,
        /// Guided setup alias for the interactive default.
        #[arg(long)]
        guided: bool,
        /// Check setup status without writing config.
        #[arg(long)]
        check: bool,
        /// Run setup doctor/status checks without writing config.
        #[arg(long)]
        doctor: bool,
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
    let root_workspace = cli.workspace.clone();
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
            cli.artifacts,
            cli.artifact_dir,
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
                workspace.unwrap_or_else(|| root_workspace.clone()),
                max_steps,
                cli.provider,
                cli.model,
                cli.agent,
                cli.json,
                cli.scripted,
                cli.artifacts,
                cli.artifact_dir,
                cli.dangerously_bypass_approvals_and_sandbox,
            ),
            Some(Command::Remember {
                title,
                content,
                memory_type,
                workspace,
            }) => run_remember(
                workspace.unwrap_or_else(|| root_workspace.clone()),
                memory_type,
                title,
                content,
            ),
            Some(Command::Recall {
                query,
                limit,
                workspace,
            }) => run_recall(
                workspace.unwrap_or_else(|| root_workspace.clone()),
                query,
                limit,
            ),
            Some(Command::Context { message, workspace }) => {
                run_context(workspace.unwrap_or_else(|| root_workspace.clone()), message)
            }
            Some(Command::ModelRequest {
                message,
                provider,
                model,
                workspace,
            }) => run_model_request(
                workspace.unwrap_or_else(|| root_workspace.clone()),
                message,
                provider,
                model,
            ),
            Some(Command::Eval { scope, file }) => run_eval(
                root_workspace.clone(),
                scope,
                file,
                cli.artifacts,
                cli.artifact_dir,
            ),
            Some(Command::Verify { scope, workspace }) => run_verify(
                workspace.unwrap_or_else(|| root_workspace.clone()),
                scope,
                cli.artifacts,
                cli.artifact_dir,
                cli.dangerously_bypass_approvals_and_sandbox,
            ),
            Some(Command::Desktop { binary, dev }) => run_desktop(binary, dev),
            Some(Command::AppServer { workspace }) => run_app_server(BridgeOptions {
                workspace: workspace.unwrap_or_else(|| root_workspace.clone()),
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
                workspace: workspace.unwrap_or_else(|| root_workspace.clone()),
                provider: cli.provider,
                model: cli.model,
                agent: cli.agent,
                dangerously_bypass_approvals_and_sandbox: cli
                    .dangerously_bypass_approvals_and_sandbox,
            }),
            Some(Command::Setup {
                status,
                guided,
                check,
                doctor,
                data_root,
                workspace,
                non_interactive,
                force,
                skip_hbse,
            }) => run_setup_command(
                status || check || doctor,
                data_root,
                workspace,
                if guided { false } else { non_interactive },
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum DesktopLaunchTarget {
    Binary(PathBuf),
    SourceDev(PathBuf),
}

fn run_desktop(binary: Option<PathBuf>, dev: bool) -> anyhow::Result<()> {
    let target = resolve_desktop_launch_target(binary, dev)?;
    match target {
        DesktopLaunchTarget::Binary(path) => {
            let mut child = std::process::Command::new(&path).spawn().map_err(|error| {
                anyhow::anyhow!(
                    "failed to launch Vegvisir Desktop at '{}': {error}",
                    path.display()
                )
            })?;
            println!(
                "launched Vegvisir Desktop: {} (pid {})",
                path.display(),
                child.id()
            );
            // Reap immediately if the process exits during startup; otherwise return after launch.
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    anyhow::bail!("Vegvisir Desktop exited during startup: {status}");
                }
            }
            Ok(())
        }
        DesktopLaunchTarget::SourceDev(desktop_dir) => {
            println!(
                "launching Vegvisir Desktop from source: {}",
                desktop_dir.display()
            );
            let status = std::process::Command::new("npm")
                .args(["run", "dev"])
                .current_dir(&desktop_dir)
                .status()
                .map_err(|error| {
                    anyhow::anyhow!(
                        "failed to run `npm run dev` in '{}': {error}",
                        desktop_dir.display()
                    )
                })?;
            if !status.success() {
                anyhow::bail!("Vegvisir Desktop dev launcher exited with status {status}");
            }
            Ok(())
        }
    }
}

fn resolve_desktop_launch_target(
    binary: Option<PathBuf>,
    dev: bool,
) -> anyhow::Result<DesktopLaunchTarget> {
    if let Some(binary) = binary {
        if is_runnable_file(&binary) {
            return Ok(DesktopLaunchTarget::Binary(binary));
        }
        anyhow::bail!(
            "desktop binary does not exist or is not a file: {}",
            binary.display()
        );
    }

    if !dev {
        if let Some(path) = std::env::var_os("VEGVISIR_DESKTOP_BINARY") {
            let path = PathBuf::from(path);
            if is_runnable_file(&path) {
                return Ok(DesktopLaunchTarget::Binary(path));
            }
        }

        if let Some(path) = find_packaged_desktop_binary()? {
            return Ok(DesktopLaunchTarget::Binary(path));
        }
    }

    if let Some(path) = find_desktop_source_dir()? {
        return Ok(DesktopLaunchTarget::SourceDev(path));
    }

    anyhow::bail!(
        "could not find Vegvisir Desktop. Set VEGVISIR_DESKTOP_BINARY, pass `vegvisir desktop --binary <path>`, or run from a source checkout containing components/desktop."
    )
}

fn find_packaged_desktop_binary() -> anyhow::Result<Option<PathBuf>> {
    let mut roots = Vec::new();
    if let Ok(current_exe) = std::env::current_exe() {
        roots.extend(ancestor_dirs(&current_exe));
    }
    roots.push(std::env::current_dir()?);

    let names = desktop_binary_names();
    for candidate in desktop_path_candidates(&names) {
        if is_runnable_file(&candidate) {
            return Ok(Some(candidate));
        }
    }
    for root in dedupe_paths(roots) {
        for candidate in desktop_binary_candidates_for_root(&root, &names) {
            if is_runnable_file(&candidate) {
                return Ok(Some(candidate));
            }
        }
    }
    Ok(None)
}

fn find_desktop_source_dir() -> anyhow::Result<Option<PathBuf>> {
    let mut roots = vec![std::env::current_dir()?];
    if let Ok(current_exe) = std::env::current_exe() {
        roots.extend(ancestor_dirs(&current_exe));
    }

    for root in dedupe_paths(roots) {
        let candidate = root.join("components").join("desktop");
        if candidate.join("package.json").is_file()
            && candidate
                .join("src-tauri")
                .join("tauri.conf.json")
                .is_file()
        {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn ancestor_dirs(path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = if path.is_dir() {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(dir) = current {
        dirs.push(dir.to_path_buf());
        current = dir.parent();
    }
    dirs
}

fn desktop_binary_candidates_for_root(root: &Path, names: &[&str]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let dirs = [
        root.to_path_buf(),
        root.join("bin"),
        root.join("resources"),
        root.join("resources").join("bin"),
        root.join("../Resources"),
        root.join("../Resources").join("bin"),
        root.join("components")
            .join("desktop")
            .join("src-tauri")
            .join("target")
            .join("release"),
        root.join("components")
            .join("desktop")
            .join("src-tauri")
            .join("target")
            .join("debug"),
    ];
    for dir in dirs {
        for name in names {
            candidates.push(dir.join(name));
        }
    }
    candidates
}

fn desktop_path_candidates(names: &[&str]) -> Vec<PathBuf> {
    let Some(paths) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for dir in std::env::split_paths(&paths) {
        for name in names {
            candidates.push(dir.join(name));
        }
    }
    candidates
}

fn desktop_binary_names() -> Vec<&'static str> {
    let mut names = vec![
        "vegvisir-desktop",
        "vegvisir_desktop",
        "Vegvisir Desktop",
        "VegvisirDesktop",
        "vegvisir-desktop.AppImage",
        "Vegvisir Desktop.AppImage",
    ];
    if cfg!(windows) {
        names.extend([
            "vegvisir-desktop.exe",
            "Vegvisir Desktop.exe",
            "VegvisirDesktop.exe",
        ]);
    }
    names
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            deduped.push(path);
        }
    }
    deduped
}

fn is_runnable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod desktop_launcher_tests {
    use super::*;

    #[test]
    fn explicit_desktop_binary_resolves_to_binary_target() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        let target = resolve_desktop_launch_target(Some(temp.path().to_path_buf()), false)
            .expect("explicit binary should resolve");
        assert_eq!(
            target,
            DesktopLaunchTarget::Binary(temp.path().to_path_buf())
        );
    }

    #[test]
    fn invalid_explicit_desktop_binary_fails() {
        let missing =
            std::env::temp_dir().join(format!("missing-vegvisir-desktop-{}", uuid::Uuid::new_v4()));
        let error = resolve_desktop_launch_target(Some(missing), false)
            .expect_err("missing explicit binary should fail");
        assert!(error.to_string().contains("desktop binary does not exist"));
    }

    #[test]
    fn desktop_binary_candidate_roots_include_packaged_and_source_build_paths() {
        let root = PathBuf::from("/opt/vegvisir");
        let names = desktop_binary_names();
        let candidates = desktop_binary_candidates_for_root(&root, &names);
        assert!(candidates.contains(&root.join("bin").join("vegvisir-desktop")));
        assert!(
            candidates.contains(
                &root
                    .join("components")
                    .join("desktop")
                    .join("src-tauri")
                    .join("target")
                    .join("release")
                    .join("vegvisir-desktop")
            )
        );
    }
}

#[allow(clippy::too_many_arguments)]
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
    artifacts: bool,
    artifact_dir: Option<PathBuf>,
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<()> {
    let mut app = TuiApplication::new_with_dangerous_bypass(
        workspace.clone(),
        dangerously_bypass_approvals_and_sandbox,
    )?;
    let mut artifact_bundle = if artifacts || artifact_dir.is_some() {
        Some(RunArtifactManager::start_in(
            &workspace,
            default_vegvisir_data_root(),
            artifact_dir.as_deref(),
            app.session.session_id.clone(),
            app.session.current_provider.clone(),
            app.session.current_model.clone(),
            app.session.active_agent_id.clone(),
        )?)
    } else {
        None
    };
    let command = format!("/verify {scope}");
    let output = app
        .execute_command(&command)?
        .unwrap_or_else(|| "No verification output.".to_string());
    if let Some((manager, mut manifest)) = artifact_bundle.take() {
        manager.write_request(&serde_json::json!({
            "scope": scope,
            "mode": "verify",
            "command": command,
        }))?;
        manager.write_result(&output)?;
        manager.write_verification_evidence(&verification_evidence_from_verify_output(
            manager.run_id.clone(),
            &command,
            &output,
        ))?;
        manager.write_memory_written_unavailable(
            "verify command does not perform completion memory writeback",
        )?;
        manager.write_approvals_from_pending(&app.tool_executor.guardrails.approvals.pending())?;
        manager.write_subagents_from_board()?;
        manager.write_workspace_change_artifacts()?;
        manager.finish(&mut manifest, RunStatus::Completed)?;
        println!("{output}");
        println!("artifact_dir: {}", manager.run_dir.display());
        return Ok(());
    }
    println!("{output}");
    Ok(())
}

fn verification_evidence_from_verify_output(
    run_id: String,
    command: &str,
    output: &str,
) -> RunVerificationEvidence {
    RunVerificationEvidence::captured(
        run_id,
        vec![RunVerificationCheck {
            name: "vegvisir_verify".to_string(),
            command: Some(command.to_string()),
            ok: Some(verify_output_passed(output)),
            summary: output
                .lines()
                .next()
                .unwrap_or("No verification output.")
                .to_string(),
            detail: Some(output.to_string()),
            source: RunVerificationSource::Harness,
        }],
    )
}

fn verify_output_passed(output: &str) -> bool {
    !output.starts_with("Usage:")
        && !output
            .lines()
            .any(|line| line.trim_start().starts_with("fail "))
}

fn run_eval(
    workspace: PathBuf,
    scope: String,
    file: Option<PathBuf>,
    artifacts: bool,
    artifact_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let mut artifact_bundle = if artifacts || artifact_dir.is_some() {
        Some(RunArtifactManager::start_in(
            &workspace,
            default_vegvisir_data_root(),
            artifact_dir.as_deref(),
            "headless-eval",
            "harness",
            "eval",
            None,
        )?)
    } else {
        None
    };
    let results = match if let Some(file) = file.as_ref() {
        run_eval_file(file)
    } else {
        run_builtin_evals(&scope)
    } {
        Ok(results) => results,
        Err(error) => {
            if let Some((manager, mut manifest)) = artifact_bundle.take() {
                manager.write_request(&serde_json::json!({
                    "scope": scope,
                    "file": file.as_ref().map(|path| path.display().to_string()),
                    "mode": "eval",
                }))?;
                manager.write_verification_unavailable(
                    "eval command failed before verification evidence could be finalized",
                )?;
                manager.write_approvals_unavailable(
                    "eval command does not expose a persistent approval ledger snapshot",
                )?;
                manager.fail(&mut manifest, error.to_string(), true)?;
            }
            return Err(error);
        }
    };
    let output = format_eval_results(&results);
    if let Some((manager, mut manifest)) = artifact_bundle.take() {
        manager.write_request(&serde_json::json!({
            "scope": scope,
            "file": file.as_ref().map(|path| path.display().to_string()),
            "mode": "eval",
        }))?;
        manager.write_result(&output)?;
        manager.write_verification_evidence(&verification_evidence_from_eval_results(
            manager.run_id.clone(),
            &scope,
            file.as_ref().map(|path| path.display().to_string()),
            &results,
        ))?;
        manager.write_memory_written_unavailable(
            "eval command does not perform completion memory writeback",
        )?;
        manager.write_approvals_unavailable(
            "eval command does not expose a persistent approval ledger snapshot",
        )?;
        manager.write_subagents_from_board()?;
        manager.write_workspace_change_artifacts()?;
        manager.finish(&mut manifest, RunStatus::Completed)?;
        println!("{output}");
        println!("artifact_dir: {}", manager.run_dir.display());
        return Ok(());
    }
    println!("{output}");
    Ok(())
}

fn verification_evidence_from_eval_results(
    run_id: String,
    scope: &str,
    file: Option<String>,
    results: &[vegvisir_rust::evals::EvalResult],
) -> RunVerificationEvidence {
    RunVerificationEvidence::captured(
        run_id,
        results
            .iter()
            .map(|result| RunVerificationCheck {
                name: format!("vegvisir_eval/{}/{}", result.category, result.id),
                command: Some(match &file {
                    Some(path) => format!("vegvisir eval --file {path}"),
                    None => format!("vegvisir eval {scope}"),
                }),
                ok: Some(result.passed),
                summary: result.details.clone(),
                detail: Some(
                    serde_json::to_string(result).unwrap_or_else(|_| result.details.clone()),
                ),
                source: RunVerificationSource::Harness,
            })
            .collect(),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_headless(
    prompt: String,
    workspace: PathBuf,
    max_steps: usize,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    json_output: bool,
    scripted: bool,
    artifacts: bool,
    artifact_dir: Option<PathBuf>,
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
            artifacts,
            artifact_dir,
            dangerously_bypass_approvals_and_sandbox,
        );
    }
    let model = ScriptedModel::default();
    let mut harness = if dangerously_bypass_approvals_and_sandbox {
        AgentHarness::with_dangerous_bypass(model, &workspace)?
    } else {
        AgentHarness::default(model, &workspace)?
    };
    let mut task = AgentTask::new(prompt.clone(), workspace.clone());
    task.max_steps = max_steps;
    let result = harness.run(task)?;
    let artifact_dir = if artifacts || artifact_dir.is_some() {
        let status = run_status_from_harness_status(&result.status);
        let (manager, mut manifest) = RunArtifactManager::start_with_run_id(
            &workspace,
            default_vegvisir_data_root(),
            result.run_id.clone(),
            artifact_dir.as_deref(),
            "headless-scripted",
            "scripted",
            "scripted-model",
            agent.clone(),
        )?;
        manager.write_request(&serde_json::json!({
            "goal": prompt,
            "max_steps": max_steps,
            "mode": "scripted_harness",
        }))?;
        let mut cms = open_cms(workspace.clone())?;
        let envelope = cms.prepare_cached_prompt(&prompt, "scripted", "scripted-model")?;
        manager.write_context_artifacts(&envelope)?;
        if let Some(answer) = result.final_answer.as_deref() {
            manager.write_result(answer)?;
            let memory_write_results = cms.complete_turn(&prompt, answer)?;
            manager.write_memory_written_from_results(&memory_write_results)?;
        } else {
            manager.write_memory_written_unavailable(
                "scripted run produced no final answer for completion memory writeback",
            )?;
        }
        manager.write_approvals_from_pending(&harness.executor.guardrails.approvals.pending())?;
        manager.write_subagents_from_board()?;
        manager.write_verification_no_checks(
            "scripted harness does not run verification commands automatically",
        )?;
        manager.write_workspace_change_artifacts()?;
        if status == RunStatus::Failed {
            let message = result.final_answer.as_deref().unwrap_or(&result.status);
            manager.write_failure(&vegvisir_rust::run_artifacts::RunFailure {
                schema_version: vegvisir_rust::run_artifacts::RUN_ARTIFACT_SCHEMA_VERSION,
                run_id: manager.run_id.clone(),
                message: message.to_string(),
                recoverable: true,
                timestamp: chrono::Utc::now(),
            })?;
        }
        manager.finish(&mut manifest, status)?;
        Some(manager.run_dir.display().to_string())
    } else {
        None
    };
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": result.status,
                "answer": result.final_answer,
                "steps": result.steps,
                "run_id": result.run_id,
                "artifact_dir": artifact_dir,
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
    if let Some(artifact_dir) = artifact_dir {
        println!("artifact_dir: {artifact_dir}");
    }
    if let Some(checkpoint) = result.checkpoint {
        println!("checkpoint: {}", checkpoint.display());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_headless_provider(
    prompt: String,
    workspace: PathBuf,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    json_output: bool,
    artifacts: bool,
    artifact_dir: Option<PathBuf>,
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<()> {
    let mut app = TuiApplication::new_with_dangerous_bypass(
        &workspace,
        dangerously_bypass_approvals_and_sandbox,
    )?;
    let mut artifact_bundle = if artifacts || artifact_dir.is_some() {
        Some(RunArtifactManager::start_in(
            &workspace,
            default_vegvisir_data_root(),
            artifact_dir.as_deref(),
            app.session.session_id.clone(),
            app.session.current_provider.clone(),
            app.session.current_model.clone(),
            app.session.active_agent_id.clone(),
        )?)
    } else {
        None
    };

    let selection_result = (|| -> anyhow::Result<()> {
        if let Some(provider) = provider {
            apply_cli_command(&mut app, &format!("/provider {provider}"), "provider")?;
        }
        if let Some(model) = model {
            apply_cli_command(&mut app, &format!("/model {model}"), "model")?;
        }
        if let Some(agent) = agent {
            apply_cli_command(&mut app, &format!("/agent use {agent}"), "agent")?;
        }
        Ok(())
    })();
    if let Err(error) = selection_result {
        if let Some((manager, mut manifest)) = artifact_bundle.take() {
            sync_manifest_selection_from_app(&mut manifest, &app);
            manager.write_request(&serde_json::json!({
                "goal": prompt,
                "mode": "provider_runtime",
            }))?;
            manager
                .write_approvals_from_pending(&app.tool_executor.guardrails.approvals.pending())?;
            manager.write_verification_unavailable(
                "provider selection failed before verification evidence could be captured",
            )?;
            manager.fail(&mut manifest, error.to_string(), true)?;
        }
        return Err(error);
    }

    if let Some((_, manifest)) = artifact_bundle.as_mut() {
        sync_manifest_selection_from_app(manifest, &app);
    }

    if json_output {
        let (observed, artifact_dir) = if let Some((manager, manifest)) = artifact_bundle {
            let (observed, run_id, dir) =
                send_provider_headless_with_artifacts(&mut app, &prompt, manager, manifest)?;
            (observed, Some((run_id, dir)))
        } else {
            (app.send_headless_observed(&prompt)?, None)
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "completed",
                "answer": observed.response,
                "workspace": app.cwd.display().to_string(),
                "session_id": app.session.session_id,
                "provider": app.session.current_provider,
                "model": app.session.current_model,
                "agent": app.session.active_agent_id,
                "messages": app.session.messages.len(),
                "tokens_used": app.session.tokens_used,
                "latency_ms": app.session.last_latency_ms,
                "prompt_cache_key": app.session.last_prompt_cache_key,
                "events": observed.events,
                "run_id": artifact_dir.as_ref().map(|(run_id, _)| run_id),
                "artifact_dir": artifact_dir.as_ref().map(|(_, dir)| dir),
                "mode": "provider_runtime",
            }))?
        );
    } else if let Some((manager, manifest)) = artifact_bundle {
        let (observed, _, artifact_dir) =
            send_provider_headless_with_artifacts(&mut app, &prompt, manager, manifest)?;
        println!("{}", observed.response);
        println!("artifact_dir: {artifact_dir}");
    } else {
        let response = app.send_headless(&prompt)?;
        println!("{response}");
    }
    Ok(())
}

fn sync_manifest_selection_from_app(manifest: &mut RunManifest, app: &TuiApplication) {
    manifest.provider = app.session.current_provider.clone();
    manifest.model = app.session.current_model.clone();
    manifest.agent = app.session.active_agent_id.clone();
}

fn run_status_from_harness_status(status: &str) -> RunStatus {
    match status {
        "completed" => RunStatus::Completed,
        "cancelled" => RunStatus::Cancelled,
        "failed" => RunStatus::Failed,
        _ => RunStatus::Failed,
    }
}

fn send_provider_headless_with_artifacts(
    app: &mut TuiApplication,
    prompt: &str,
    manager: RunArtifactManager,
    mut manifest: RunManifest,
) -> anyhow::Result<(vegvisir_rust::app::HeadlessObservedRun, String, String)> {
    match app.send_headless_observed(prompt) {
        Ok(observed) => {
            let pending_approvals = app.tool_executor.guardrails.approvals.pending();
            write_provider_headless_artifacts(
                &manager,
                &mut manifest,
                prompt,
                &observed,
                &pending_approvals,
            )?;
            Ok((
                observed,
                manager.run_id.clone(),
                manager.run_dir.display().to_string(),
            ))
        }
        Err(error) => {
            manager.write_request(&serde_json::json!({
                "goal": prompt,
                "mode": "provider_runtime",
            }))?;
            manager
                .write_approvals_from_pending(&app.tool_executor.guardrails.approvals.pending())?;
            manager.write_verification_unavailable(
                "provider run failed before verification evidence could be finalized",
            )?;
            manager.fail(&mut manifest, error.to_string(), true)?;
            Err(error)
        }
    }
}

fn write_provider_headless_artifacts(
    manager: &RunArtifactManager,
    manifest: &mut RunManifest,
    prompt: &str,
    observed: &vegvisir_rust::app::HeadlessObservedRun,
    pending_approvals: &std::collections::BTreeMap<
        String,
        vegvisir_rust::guardrails::ApprovalRequest,
    >,
) -> anyhow::Result<()> {
    manager.write_request(&serde_json::json!({
        "goal": prompt,
        "mode": "provider_runtime",
    }))?;
    manager.write_context_artifacts(&observed.prompt_envelope)?;
    manager.write_result(&observed.response)?;
    manager.write_memory_written_from_outcome(
        &observed.memory_write_results,
        observed.memory_write_error.as_deref(),
    )?;
    manager.write_approvals_from_pending(pending_approvals)?;
    manager.write_subagents_from_board()?;
    manager.write_workspace_change_artifacts()?;
    for event in &observed.events {
        manager.append_observed_provider_event(event)?;
    }
    manager.finish(manifest, RunStatus::Completed)
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
