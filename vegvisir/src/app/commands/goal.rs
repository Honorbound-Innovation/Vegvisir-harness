use std::path::{Path, PathBuf};

use super::super::*;
use super::autonomy_plan::{
    current_autonomy_node_slices, read_autonomy_plan_status, write_autonomy_libraries,
};

const GOAL_DIR: &str = ".vegvisir/goal";

impl TuiApplication {
    pub(crate) fn goal_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("status") | Some("show") => self.goal_status(),
            Some("start") | Some("run") => self.start_goal(args.get(1..).unwrap_or_default()),
            Some("stop") | Some("cancel") => self.stop_goal(),
            Some("resume") => match args.get(1) {
                Some(path) => self.start_goal(&[path.clone()]),
                None => "Usage: /goal resume <spec.md>".to_string(),
            },
            Some(_path) => self.start_goal(args),
        }
    }

    fn start_goal(&mut self, args: &[String]) -> String {
        if self.goal.active || self.pending_send.is_some() {
            return "Goal mode cannot start while another goal or model turn is active. Use /goal status or /goal stop first.".to_string();
        }
        let raw_path = args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        if raw_path.trim().is_empty() {
            return "Usage: /goal start <specification.md>".to_string();
        }
        let spec_path = PathBuf::from(raw_path.trim());
        if spec_path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
        {
            return "Goal mode specification path must not contain `..`.".to_string();
        }
        if spec_path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            return "Goal mode requires a Markdown specification file (*.md).".to_string();
        }
        let absolute_spec = if spec_path.is_absolute() {
            spec_path.clone()
        } else {
            self.cwd.join(&spec_path)
        };
        let workspace_root = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());
        let Ok(canonical_spec) = absolute_spec.canonicalize() else {
            return format!(
                "Goal mode could not read specification `{}`.",
                spec_path.display()
            );
        };
        if canonical_spec.strip_prefix(&workspace_root).is_err() {
            return "Goal mode specification must be inside the active workspace.".to_string();
        }
        let Ok(spec) = std::fs::read_to_string(&canonical_spec) else {
            return format!(
                "Goal mode could not read specification `{}`.",
                spec_path.display()
            );
        };
        if spec.trim().is_empty() {
            return format!(
                "Goal mode requires a non-empty specification `{}`.",
                spec_path.display()
            );
        }

        let spec_display = relative_or_display(&workspace_root, &canonical_spec);
        let safe_session = self
            .session
            .session_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let plan_path = format!("{GOAL_DIR}/{safe_session}-plan.md");
        self.goal = GoalState {
            enabled: true,
            active: true,
            spec_path: Some(spec_display.clone()),
            plan_path: Some(plan_path.clone()),
            objective: format!("Implement the complete specification in `{spec_display}`"),
            step: 1,
            last_status: "planning: awaiting complete implementation plan".to_string(),
            ..GoalState::default()
        };
        self.push_system_message(format!(
            "Goal mode started for `{spec_display}`. It will continue until the specification exit criteria are satisfied, or you stop it, cancel it, or a policy-required blocker prevents progress."
        ));
        self.logger.emit(
            "goal_start",
            json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "spec_path": spec_display,
                "plan_path": plan_path,
            }),
        );
        let prompt = self.goal_initial_prompt(&spec_display, &plan_path, spec.len());
        self.start_background_send(prompt, Vec::new());
        "Goal mode is running. The model must read the specification, write the end-to-end plan, implement it, and verify every exit criterion before completion.".to_string()
    }

    fn goal_status(&self) -> String {
        format!(
            "Goal mode\nenabled={}\nactive={}\nstatus={}\nstep={}\nspec_path={}\nplan_path={}\ncurrent_node={}\nnode_progress={}/{}\nchecklist={}/{}\nevidence_status={}\nevidence_valid={}\nexit_criteria_complete={}",
            self.goal.enabled,
            self.goal.active,
            self.goal.last_status,
            self.goal.step,
            self.goal.spec_path.as_deref().unwrap_or("-"),
            self.goal.plan_path.as_deref().unwrap_or("-"),
            self.goal.current_node_id.as_deref().unwrap_or("-"),
            self.goal.node_completed,
            self.goal.node_total,
            self.goal.checklist_completed,
            self.goal.checklist_total,
            self.goal.current_evidence_status.as_deref().unwrap_or("-"),
            self.goal.current_evidence_valid,
            self.goal.exit_criteria_complete,
        )
    }

    fn stop_goal(&mut self) -> String {
        if !self.goal.active && !self.goal.enabled {
            return "Goal mode is not running.".to_string();
        }
        if self.pending_send.is_some() {
            let cancelled = self.cancel_pending_response();
            self.goal.enabled = false;
            self.goal.active = false;
            self.goal.last_status = "stopped_by_user".to_string();
            return format!("Goal mode stopped. {cancelled}");
        }
        self.finish_goal("stopped_by_user");
        "Goal mode stopped.".to_string()
    }

    pub(crate) fn poll_goal_controller(&mut self) -> bool {
        if !self.goal.enabled || !self.goal.active || self.pending_send.is_some() {
            return false;
        }
        if self.tool_executor.guardrails.approvals.pending_len() > 0 {
            self.finish_goal("blocked: pending tool approval");
            return true;
        }

        let Some(plan_path_text) = self.goal.plan_path.clone() else {
            self.goal.last_status = "planning: missing plan path".to_string();
            self.start_background_send(
                "Goal mode is still planning. Create the complete Markdown implementation plan before taking implementation actions.".to_string(),
                Vec::new(),
            );
            return true;
        };
        let plan_path = Path::new(&plan_path_text);
        if !self.cwd.join(plan_path).exists() {
            self.goal.last_status = "planning: plan not written yet".to_string();
            self.goal.step = self.goal.step.saturating_add(1);
            self.push_system_message(format!(
                "Goal mode continuing step {}: the end-to-end plan has not been written yet.",
                self.goal.step
            ));
            self.start_background_send(
                self.goal_initial_prompt(
                    self.goal.spec_path.as_deref().unwrap_or("spec.md"),
                    &plan_path_text,
                    0,
                ),
                Vec::new(),
            );
            return true;
        }

        if let Err(error) = write_autonomy_libraries(
            &self.cwd,
            plan_path,
            &self.goal.objective,
            &self.session.session_id,
        ) {
            self.goal.last_status = format!("planning: plan compilation failed: {error}");
            self.start_background_send(
                self.goal_compile_repair_prompt(&plan_path_text),
                Vec::new(),
            );
            return true;
        }

        let status = match read_autonomy_plan_status(&self.cwd, plan_path, &self.goal.objective) {
            Ok(Some(status)) => status,
            Ok(None) => {
                self.start_background_send(
                    self.goal_compile_repair_prompt(&plan_path_text),
                    Vec::new(),
                );
                return true;
            }
            Err(error) => {
                self.goal.last_status = format!("verification error: {error}");
                self.start_background_send(
                    self.goal_compile_repair_prompt(&plan_path_text),
                    Vec::new(),
                );
                return true;
            }
        };
        self.goal.node_total = status.total_nodes;
        self.goal.node_completed = status.completed_nodes;
        self.goal.checklist_total = status.nodes.iter().map(|node| node.checklist_total).sum();
        self.goal.checklist_completed = status
            .nodes
            .iter()
            .map(|node| node.checklist_completed)
            .sum();
        self.goal.current_node_id = status.current_node_id.clone();
        self.goal.current_node_title = status.current_node_title.clone();
        self.goal.current_evidence_path = status.current_evidence_path.clone();
        self.goal.current_evidence_status = status.current_evidence_status.clone();
        self.goal.current_evidence_valid = status.current_evidence_valid;
        self.goal.current_evidence_blocked = status.current_evidence_blocked;
        self.goal.exit_criteria_complete = self.goal.checklist_total > 0
            && self.goal.checklist_completed == self.goal.checklist_total
            && status.total_nodes > 0
            && status.completed_nodes == status.total_nodes;
        if status.current_evidence_blocked {
            self.finish_goal("blocked: current node reported blocker evidence");
            return true;
        }
        if self.goal.exit_criteria_complete {
            self.finish_goal("completed: all specification exit criteria have validated evidence");
            return true;
        }

        let slices = current_autonomy_node_slices(&self.cwd, plan_path, &self.goal.objective)
            .ok()
            .flatten()
            .map(|(_, cll, pll)| (cll, pll))
            .unwrap_or_else(|| {
                (
                    "Current plan node could not be loaded.".to_string(),
                    "Current prompt slice could not be loaded.".to_string(),
                )
            });
        self.goal.step = self.goal.step.saturating_add(1);
        self.goal.last_status = format!(
            "implementing step {}; exit criteria {}/{}; nodes {}/{}",
            self.goal.step,
            self.goal.checklist_completed,
            self.goal.checklist_total,
            self.goal.node_completed,
            self.goal.node_total
        );
        self.push_system_message(format!(
            "Goal mode continuing step {}: exit criteria {}/{} complete; nodes {}/{}.",
            self.goal.step,
            self.goal.checklist_completed,
            self.goal.checklist_total,
            self.goal.node_completed,
            self.goal.node_total
        ));
        self.start_background_send(
            self.goal_continue_prompt(&plan_path_text, &slices.0, &slices.1),
            Vec::new(),
        );
        true
    }

    fn finish_goal(&mut self, status: &str) {
        self.goal.active = false;
        self.goal.enabled = false;
        self.goal.last_status = status.to_string();
        self.push_system_message(format!("Goal mode stopped: {status}."));
        self.logger.emit(
            "goal_stop",
            json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "status": status,
                "step": self.goal.step,
                "spec_path": self.goal.spec_path,
                "plan_path": self.goal.plan_path,
                "node_total": self.goal.node_total,
                "node_completed": self.goal.node_completed,
                "checklist_total": self.goal.checklist_total,
                "checklist_completed": self.goal.checklist_completed,
            }),
        );
        self.autosave_session();
        self.redraw_requested = true;
    }

    pub(crate) fn goal_cancelled(&mut self, status: &str) {
        if self.goal.active {
            self.goal.active = false;
            self.goal.enabled = false;
            self.goal.last_status = status.to_string();
        }
    }

    fn goal_initial_prompt(&self, spec_path: &str, plan_path: &str, spec_bytes: usize) -> String {
        format!(
            "Goal mode specification implementation. Read the complete Markdown specification at `{spec_path}` before acting (it is {spec_bytes} bytes). Implement the entire specification end to end; do not stop after a sample, a fixed number of steps, or a partial milestone.\n\nGoal mode contract:\n1. Create or overwrite the complete implementation plan at `{plan_path}`.\n2. Derive the plan from every requirement and explicit exit/acceptance criterion in `{spec_path}`. Include headings for phases, Success conditions, Expected deliverables, Implementation rules, Guardrails, Validation, and a Markdown checklist (`- [ ]` / `- [x]`).\n3. Then implement every planned phase in the workspace. Keep the plan checklist accurate and only check an item after it is actually complete.\n4. Run the validations required by the specification and fix failures. For each plan node, write the validated JSON completion evidence packet beside the plan under the generated evidence directory.\n5. Do not claim completion merely because code was written or because one test passed. Goal mode will continue automatically until every planned exit criterion has a checked checklist item and validated evidence.\n6. Continue through routine inspect/implement/test/fix iterations without asking for confirmation. Stop only for user cancellation, a policy-required approval/blocker, an unrecoverable provider failure, or after the harness verifies all exit criteria.\n\nTake the first action now: read `{spec_path}` and write the full plan to `{plan_path}`.",
        )
    }

    fn goal_continue_prompt(&self, plan_path: &str, cll: &str, pll: &str) -> String {
        format!(
            "Continue Goal mode. The source specification is `{}` and the controlling plan is `{plan_path}`. This is an unbounded goal run: do not stop because a turn, milestone, or arbitrary step count ended.\n\nCurrent harness progress: step {}; exit criteria checklist {}/{}; validated nodes {}/{}; current node `{}` ({}) with evidence `{}` and status `{}`.\n\nCLL slice (task-local user context):\n{cll}\n\nPLL slice (task-local user context):\n{pll}\n\nRequired next action: inspect the current workspace and specification-derived plan, implement or repair the current node, run its required validations, write/update its completion evidence packet, and update `{plan_path}`. Continue until the harness reports every exit criterion complete. Do not mark checklist items or evidence complete without actual deliverables and verification.",
            self.goal.spec_path.as_deref().unwrap_or("spec.md"),
            self.goal.step,
            self.goal.checklist_completed,
            self.goal.checklist_total,
            self.goal.node_completed,
            self.goal.node_total,
            self.goal.current_node_id.as_deref().unwrap_or("-"),
            self.goal.current_node_title.as_deref().unwrap_or("-"),
            self.goal.current_evidence_path.as_deref().unwrap_or("-"),
            self.goal.current_evidence_status.as_deref().unwrap_or("-"),
        )
    }

    fn goal_compile_repair_prompt(&self, plan_path: &str) -> String {
        format!(
            "Goal mode cannot yet verify the controlling plan `{plan_path}`. Read the specification `{}` and repair or complete the Markdown plan, including every exit criterion, checklist item, deliverable, and validation. Then continue implementation; do not stop at planning.",
            self.goal.spec_path.as_deref().unwrap_or("spec.md")
        )
    }
}

fn relative_or_display(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::commands::autonomy_plan::evidence_packet_path;

    #[test]
    fn goal_command_rejects_non_markdown_and_out_of_workspace_specs() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        assert!(
            app.goal_command(&["start".to_string(), "spec.txt".to_string()])
                .contains("Markdown")
        );
        assert!(
            app.goal_command(&["start".to_string(), "/tmp/spec.md".to_string()])
                .contains("could not read")
        );
        Ok(())
    }

    #[test]
    fn relative_or_display_prefers_workspace_relative_paths() {
        let cwd = Path::new("/workspace");
        assert_eq!(relative_or_display(cwd, &cwd.join("spec.md")), "spec.md");
        assert_eq!(
            relative_or_display(cwd, Path::new("/tmp/spec.md")),
            "/tmp/spec.md"
        );
    }

    #[test]
    fn goal_controller_finishes_only_after_checked_plan_and_valid_evidence() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let plan_path = Path::new(".vegvisir/goal/test-plan.md");
        std::fs::create_dir_all(tmp.path().join(".vegvisir/goal"))?;
        std::fs::write(
            tmp.path().join(plan_path),
            "# Goal\n- [x] implement and verify\n",
        )?;
        let paths = write_autonomy_libraries(tmp.path(), plan_path, "test goal", "test-run")?;
        let status = read_autonomy_plan_status(tmp.path(), plan_path, "test goal")?.unwrap();
        let node_id = status.current_node_id.as_deref().unwrap();
        std::fs::write(
            tmp.path()
                .join(evidence_packet_path(&paths.evidence_dir, node_id)),
            serde_json::json!({
                "node_id": node_id,
                "status": "complete",
                "actions_taken": ["implemented and verified"],
                "deliverables": [],
                "success_conditions_satisfied": [],
                "verification": [],
                "risks_or_blockers": [],
                "next_recommended_action": null
            })
            .to_string(),
        )?;
        app.goal = GoalState {
            enabled: true,
            active: true,
            plan_path: Some(plan_path.display().to_string()),
            objective: "test goal".to_string(),
            ..GoalState::default()
        };

        assert!(app.poll_goal_controller());
        assert!(!app.goal.active);
        assert_eq!(
            app.goal.last_status,
            "completed: all specification exit criteria have validated evidence"
        );
        Ok(())
    }
}
