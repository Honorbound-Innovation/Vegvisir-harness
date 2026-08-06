use std::path::Path;

use super::super::*;

impl TuiApplication {
    pub(crate) fn acp_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("help") => Ok(acp_help()),
            Some("init") => self.acp_init(args.get(1..).unwrap_or_default()),
            Some("status") | Some("show") => {
                Ok(crate::acp::AcpSnapshot::load(&self.cwd)?.status_report())
            }
            Some("validate") | Some("check") => {
                let snapshot = crate::acp::AcpSnapshot::load(&self.cwd)?;
                let validation = snapshot.validation();
                let mut output = if validation.is_valid() {
                    "ACP validation: ok".to_string()
                } else {
                    "ACP validation: failed".to_string()
                };
                for error in validation.errors {
                    output.push_str(&format!("\nerror={error}"));
                }
                for warning in validation.warnings {
                    output.push_str(&format!("\nwarning={warning}"));
                }
                Ok(output)
            }
            Some("context") => Ok(crate::acp::AcpSnapshot::load(&self.cwd)?.render_context()),
            Some("list") | Some("commands") => {
                let snapshot = crate::acp::AcpSnapshot::load(&self.cwd)?;
                let names = snapshot.command_names();
                if names.is_empty() {
                    Ok("No ACP command documents found under agent/commands/.".to_string())
                } else {
                    Ok(format!("ACP command documents\n{}", names.join("\n")))
                }
            }
            Some("show-command") | Some("show-command-file") => {
                let Some(name) = args.get(1) else {
                    return Ok("Usage: /acp show-command <name>".to_string());
                };
                let (document, path) = crate::acp::read_command_document(&self.cwd, name)?;
                Ok(format!("ACP command: {}\n\n{document}", path.display()))
            }
            Some("run") | Some("execute") => self.acp_run(args.get(1..).unwrap_or_default()),
            Some(command) if command.starts_with("@acp.") => self.acp_run(&args.to_vec()),
            Some(unknown) => Ok(format!(
                "Unknown ACP subcommand `{unknown}`. Use `/acp help` for supported operations."
            )),
        }
    }

    fn acp_init(&mut self, args: &[String]) -> anyhow::Result<String> {
        let force = args.iter().any(|arg| arg == "--force" || arg == "-f");
        let report = crate::acp::initialize(&self.cwd, force)?;
        let created = report
            .created
            .iter()
            .map(|path| relative_path(&self.cwd, path))
            .collect::<Vec<_>>();
        let skipped = report
            .skipped
            .iter()
            .map(|path| relative_path(&self.cwd, path))
            .collect::<Vec<_>>();
        let mut output = format!(
            "ACP initialized in {}.\ncreated={} skipped={}",
            self.cwd.display(),
            created.len(),
            skipped.len()
        );
        if !created.is_empty() {
            output.push_str("\ncreated_files=");
            output.push_str(&created.join(","));
        }
        if !skipped.is_empty() {
            output.push_str("\nskipped_existing=");
            output.push_str(&skipped.join(","));
        }
        self.push_system_message(
            "ACP workspace structure is ready. Use `/acp status` or `/acp validate` to inspect it.",
        );
        Ok(output)
    }

    fn acp_run(&mut self, args: &[String]) -> anyhow::Result<String> {
        if self.pending_send.is_some() {
            return Ok(
                "An ACP command cannot start while another model turn is active. Use /cancel or wait for the current turn."
                    .to_string(),
            );
        }
        let Some(raw_name) = args.first() else {
            return Ok("Usage: /acp run <command-name> [arguments...]".to_string());
        };
        let command_name = if raw_name.starts_with("@") {
            raw_name.clone()
        } else if raw_name.contains('.') {
            format!("@{raw_name}")
        } else {
            format!("@acp.{raw_name}")
        };
        let invocation = std::iter::once(command_name.as_str())
            .chain(args.iter().skip(1).map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let prompt = crate::acp::expand_command_invocation(&self.cwd, &invocation)?
            .ok_or_else(|| anyhow::anyhow!("invalid ACP command invocation"))?;
        self.start_background_send_with_display(prompt, invocation.clone(), Vec::new());
        Ok(format!("Started ACP command {invocation}."))
    }
}

fn relative_path(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn acp_help() -> String {
    "Agent Context Protocol (ACP)\n\n\
/acp init [--force]       create the portable ACP directory pattern\n\
/acp status               show discovered ACP files and progress\n\
/acp validate             validate AGENT.md, progress.yaml, and the directory pattern\n\
/acp context              render bounded ACP context used for model turns\n\
/acp list                 list command documents under agent/commands/\n\
/acp show-command <name>  print one command document\n\
/acp run <name> [args...] load and run a command document as a normal model turn\n\
\
The external ACP spelling is also supported: type @acp.status, @acp.resume, or another command document name. ACP documents are context, not executable shell code; normal Vegvisir safety, approval, sandbox, and secret policies remain authoritative."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_command_initializes_and_reports_workspace() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(temp.path(), temp.path().join("home"))?;
        let initialized = app.acp_command(&["init".to_string()])?;
        assert!(initialized.contains("ACP initialized"));
        let status = app.acp_command(&["status".to_string()])?;
        assert!(status.contains("initialized=true"));
        assert!(status.contains("commands=8"));
        let validation = app.acp_command(&["validate".to_string()])?;
        assert!(validation.starts_with("ACP validation: ok"));
        Ok(())
    }

    #[test]
    fn acp_command_lists_and_shows_command_documents() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(temp.path(), temp.path().join("home"))?;
        app.acp_command(&["init".to_string()])?;
        let list = app.acp_command(&["list".to_string()])?;
        assert!(list.contains("acp.status"));
        let show = app.acp_command(&["show-command".to_string(), "status".to_string()])?;
        assert!(show.contains("ACP command: "));
        assert!(show.contains("workspace-authored"));
        Ok(())
    }
}
