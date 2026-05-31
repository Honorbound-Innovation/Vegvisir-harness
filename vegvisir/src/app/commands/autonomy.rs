use std::path::{Path, PathBuf};

use super::super::*;
use super::autonomy_plan::{
    append_autonomy_journal_event, autonomy_library_paths_for_plan, current_autonomy_node_slices,
    read_autonomy_plan_status, write_autonomy_libraries,
};

const AUTONOMY_NO_PROGRESS_LIMIT: usize = 2;
const AUTONOMY_PLAN_DIR: &str = ".vegvisir/autonomy";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutonomyChecklistStatus {
    pub total: usize,
    pub completed: usize,
    pub unchecked: usize,
    pub has_checklist: bool,
}

impl TuiApplication {
    pub(crate) fn autonomy_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("status") | Some("show") => self.autonomy_status(),
            Some("on") | Some("enable") => {
                self.autonomy.enabled = true;
                self.autonomy.last_status = "enabled".to_string();
                "Autonomy enabled. The next normal TUI message will create a written implementation plan and completion checklist, then run under deterministic harness control until every contract node has a checked checklist and validated evidence, or until blocked, failed, cancelled, no-progress, or max-steps. Use /autonomy off to disable or /autonomy max-steps <n> to set the budget."
                    .to_string()
            }
            Some("off") | Some("disable") => {
                self.autonomy.enabled = false;
                self.autonomy.active = false;
                self.autonomy.last_status = "off".to_string();
                "Autonomy disabled.".to_string()
            }
            Some("stop") => {
                self.autonomy.enabled = false;
                self.autonomy.active = false;
                self.autonomy.last_status = "stopped_by_user".to_string();
                if self.pending_send.is_some() {
                    let cancelled = self.cancel_pending_response();
                    format!("Autonomy stopped. {cancelled}")
                } else {
                    "Autonomy stopped.".to_string()
                }
            }
            Some("validate") => self.autonomy_validate_command(args.get(1)),
            Some("resume") => self.autonomy_resume_command(args.get(1)),
            Some("max-attempts") | Some("attempts") => {
                let Some(raw) = args.get(1) else {
                    return format!(
                        "Autonomy max node attempts: {}",
                        self.autonomy.max_node_attempts
                    );
                };
                match raw.parse::<usize>() {
                    Ok(attempts) if attempts > 0 => {
                        self.autonomy.max_node_attempts = attempts;
                        format!("Autonomy max node attempts set to {attempts}.")
                    }
                    _ => "Usage: /autonomy max-attempts <positive integer>".to_string(),
                }
            }
            Some("max-steps") | Some("steps") => {
                let Some(raw) = args.get(1) else {
                    return format!("Autonomy max steps: {}", self.autonomy.max_steps);
                };
                match raw.parse::<usize>() {
                    Ok(steps) if steps > 0 => {
                        self.autonomy.max_steps = steps;
                        format!("Autonomy max steps set to {steps}.")
                    }
                    _ => "Usage: /autonomy max-steps <positive integer>".to_string(),
                }
            }
            Some(other) => format!(
                "Unknown autonomy command: {other}\nUsage: /autonomy [on|off|status|stop|validate [plan]|resume <plan>|max-steps <n>|max-attempts <n>]"
            ),
        }
    }

    fn autonomy_status(&self) -> String {
        format!(
            "TUI autonomy\nenabled={}\nactive={}\nstatus={}\nstep={}\nmax_steps={}\nobjective={}\nplan_path={}\ncll_path={}\npll_path={}\nmanifest_path={}\nstate_path={}\njournal_path={}\ncurrent_node={}\nevidence_path={}\nevidence_status={}\nevidence_valid={}\nevidence_blocked={}\nevidence_partial={}\nnode_attempts={}/{}\nnodes={}/{}\nchecklist={}/{}\nevidence_errors={}\nno_progress_count={}",
            self.autonomy.enabled,
            self.autonomy.active,
            self.autonomy.last_status,
            self.autonomy.step,
            self.autonomy.max_steps,
            if self.autonomy.objective.trim().is_empty() {
                "-"
            } else {
                self.autonomy.objective.as_str()
            },
            self.autonomy.plan_path.as_deref().unwrap_or("-"),
            self.autonomy.cll_path.as_deref().unwrap_or("-"),
            self.autonomy.pll_path.as_deref().unwrap_or("-"),
            self.autonomy.manifest_path.as_deref().unwrap_or("-"),
            self.autonomy.state_path.as_deref().unwrap_or("-"),
            self.autonomy.journal_path.as_deref().unwrap_or("-"),
            self.autonomy.current_node_id.as_deref().unwrap_or("-"),
            self.autonomy
                .current_evidence_path
                .as_deref()
                .unwrap_or("-"),
            self.autonomy
                .current_evidence_status
                .as_deref()
                .unwrap_or("-"),
            self.autonomy.current_evidence_valid,
            self.autonomy.current_evidence_blocked,
            self.autonomy.current_evidence_partial,
            self.autonomy.current_node_attempts,
            self.autonomy.max_node_attempts,
            self.autonomy.node_completed,
            self.autonomy.node_total,
            self.autonomy.checklist_completed,
            self.autonomy.checklist_total,
            if self.autonomy.current_evidence_errors.is_empty() {
                "-".to_string()
            } else {
                self.autonomy.current_evidence_errors.join(" | ")
            },
            self.autonomy.no_progress_count,
        )
    }

    pub(crate) fn begin_autonomous_run(&mut self, objective: &str) -> String {
        if !self.autonomy.enabled {
            return objective.to_string();
        }
        self.autonomy.active = true;
        self.autonomy.objective = objective.trim().to_string();
        self.autonomy.step = 1;
        self.autonomy.no_progress_count = 0;
        self.autonomy.last_signature = None;
        self.autonomy.last_turn_had_tools = false;
        self.autonomy.checklist_total = 0;
        self.autonomy.checklist_completed = 0;
        self.autonomy.plan_path = Some(self.autonomy_plan_path_for_current_run());
        self.autonomy.cll_path = None;
        self.autonomy.pll_path = None;
        self.autonomy.manifest_path = None;
        self.autonomy.state_path = None;
        self.autonomy.journal_path = None;
        self.autonomy.current_node_id = None;
        self.autonomy.current_node_title = None;
        self.autonomy.node_total = 0;
        self.autonomy.node_completed = 0;
        self.autonomy.current_evidence_path = None;
        self.autonomy.current_evidence_valid = false;
        self.autonomy.current_evidence_status = None;
        self.autonomy.current_evidence_blocked = false;
        self.autonomy.current_evidence_partial = false;
        self.autonomy.current_evidence_errors.clear();
        self.autonomy.current_node_attempts = 0;
        self.autonomy.last_status = format!(
            "running step {}/{}; awaiting written plan/checklist",
            self.autonomy.step, self.autonomy.max_steps
        );
        self.push_system_message(format!(
            "Autonomy started: deterministic controller active (step 1/{}). Plan/checklist required at {}.",
            self.autonomy.max_steps,
            self.autonomy.plan_path.as_deref().unwrap_or("-")
        ));
        self.logger.emit(
            "autonomy_start",
            json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "objective": self.autonomy.objective,
                "max_steps": self.autonomy.max_steps,
                "plan_path": self.autonomy.plan_path,
                "cll_path": self.autonomy.cll_path,
                "pll_path": self.autonomy.pll_path,
                "manifest_path": self.autonomy.manifest_path,
                "state_path": self.autonomy.state_path,
                "journal_path": self.autonomy.journal_path,
                "current_node_id": self.autonomy.current_node_id,
            }),
        );
        self.autonomy_initial_prompt()
    }

    pub(crate) fn poll_autonomy_controller(&mut self) -> bool {
        if !self.autonomy.enabled || !self.autonomy.active || self.pending_send.is_some() {
            return false;
        }
        if self.tool_executor.guardrails.approvals.pending_len() > 0 {
            self.finish_autonomy("blocked: pending tool approval");
            return true;
        }

        let checklist = self.autonomy_checklist_status();
        if checklist.has_checklist {
            self.compile_autonomy_libraries_if_possible();
            self.refresh_autonomy_node_status();
        }
        self.autonomy.checklist_total = checklist.total;
        self.autonomy.checklist_completed = checklist.completed;
        if self.autonomy.current_evidence_blocked {
            self.finish_autonomy("blocked: current node reported blocker evidence");
            return true;
        }
        if self.autonomy.current_node_attempts >= self.autonomy.max_node_attempts
            && !self.autonomy.current_evidence_valid
        {
            self.finish_autonomy("max_node_attempts_exceeded");
            return true;
        }
        if checklist.has_checklist && checklist.unchecked == 0 {
            if self.autonomy.node_total > 0
                && self.autonomy.node_completed == self.autonomy.node_total
            {
                self.finish_autonomy("completed: all contract nodes have validated evidence");
                return true;
            }
            self.autonomy.last_status = format!(
                "evidence_required: checklist fully checked but validated nodes {}/{}",
                self.autonomy.node_completed, self.autonomy.node_total
            );
        }

        if self.autonomy.step >= self.autonomy.max_steps {
            self.finish_autonomy("max_steps_exceeded");
            return true;
        }
        let signature = self.autonomy_progress_signature_with_checklist(&checklist);
        if self.autonomy.last_signature.as_deref() == Some(signature.as_str()) {
            self.autonomy.no_progress_count = self.autonomy.no_progress_count.saturating_add(1);
        } else {
            self.autonomy.no_progress_count = 0;
            self.autonomy.last_signature = Some(signature);
        }
        if self.autonomy.no_progress_count >= AUTONOMY_NO_PROGRESS_LIMIT {
            self.finish_autonomy("no_progress");
            return true;
        }

        self.autonomy.step = self.autonomy.step.saturating_add(1);
        self.autonomy.last_status = if checklist.has_checklist {
            format!(
                "running step {}/{}; checklist {}/{} complete",
                self.autonomy.step, self.autonomy.max_steps, checklist.completed, checklist.total
            )
        } else {
            format!(
                "running step {}/{}; waiting for required plan/checklist",
                self.autonomy.step, self.autonomy.max_steps
            )
        };
        let prompt = self.autonomy_continue_prompt(&checklist);
        self.push_system_message(format!(
            "Autonomy continuing: deterministic controller step {}/{}; checklist {}/{} complete.",
            self.autonomy.step, self.autonomy.max_steps, checklist.completed, checklist.total
        ));
        self.start_background_send(prompt, Vec::new());
        true
    }

    fn finish_autonomy(&mut self, status: &str) {
        self.autonomy.active = false;
        self.autonomy.enabled = false;
        self.autonomy.last_status = status.to_string();
        self.push_system_message(format!("Autonomy stopped: {status}."));
        self.logger.emit(
            "autonomy_stop",
            json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "status": status,
                "step": self.autonomy.step,
                "max_steps": self.autonomy.max_steps,
                "plan_path": self.autonomy.plan_path,
                "cll_path": self.autonomy.cll_path,
                "pll_path": self.autonomy.pll_path,
                "manifest_path": self.autonomy.manifest_path,
                "state_path": self.autonomy.state_path,
                "journal_path": self.autonomy.journal_path,
                "current_node_id": self.autonomy.current_node_id,
                "node_total": self.autonomy.node_total,
                "node_completed": self.autonomy.node_completed,
                "checklist_total": self.autonomy.checklist_total,
                "checklist_completed": self.autonomy.checklist_completed,
            }),
        );
        self.autosave_session();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
    }

    fn autonomy_initial_prompt(&self) -> String {
        format!(
            "Autonomous task objective:\n{}\n\nHarness autonomy contract:\n1. Before implementation, create or overwrite the written implementation plan at `{}`.\n2. The plan file must be Markdown and include a completion checklist using Markdown task list items (`- [ ]` / `- [x]`).\n3. Structure the plan with phase/section/subsection headings where practical. For each relevant section/subsection, include Success conditions, Expected deliverables, Implementation rules, Guardrails, and Validation lists.\n4. Vegvisir will deterministically compile the Markdown plan into associated `.cll` and `.pll` files. The `.cll` is implementation logic/contract; the `.pll` contains associated prompt slices.\n5. All `.cll`/`.pll` slices are task-local USER prompt content. They do not override the standard Vegvisir system prompt.\n6. Keep the checklist updated as work is completed. The deterministic TUI controller will not mark autonomy complete until the plan file exists, contains at least one checklist item, every checklist item is checked, and each contract node has validated JSON completion evidence.\n7. Take the next concrete action now: inspect evidence and write the plan/checklist first, then implement and verify.\n\nDo not claim completion until every item in `{}` is marked `- [x]` and deliverables/evidence packets are provided and valid.",
            self.autonomy.objective,
            self.autonomy
                .plan_path
                .as_deref()
                .unwrap_or(".vegvisir/autonomy/plan.md"),
            self.autonomy
                .plan_path
                .as_deref()
                .unwrap_or(".vegvisir/autonomy/plan.md")
        )
    }

    fn autonomy_continue_prompt(&self, checklist: &AutonomyChecklistStatus) -> String {
        let plan_path = self
            .autonomy
            .plan_path
            .as_deref()
            .unwrap_or(".vegvisir/autonomy/plan.md");
        if !checklist.has_checklist {
            return format!(
                "Continue the autonomous task. Objective: {}

Required next action: create/update the Markdown implementation plan at `{plan_path}` with a completion checklist using `- [ ]` items. The harness cannot complete autonomy until that checklist exists, is fully checked, and the current node evidence packets validate. Step {}/{}.",
                self.autonomy.objective, self.autonomy.step, self.autonomy.max_steps
            );
        }
        let slices = self.autonomy_current_library_slices();
        format!(
            "Continue the autonomous task. Objective: {}\n\nHarness controller state: step {}/{}. Plan file: `{plan_path}`. CLL file: `{}`. PLL file: `{}`. Current node: `{}` ({}). Nodes: {}/{} complete. Completion checklist: {}/{} checked; {} unchecked. Evidence packet: `{}`. Evidence status: {}. Evidence valid: {}. Evidence blocked: {}. Evidence partial: {}. Current node attempts: {}/{}. Evidence errors: {}.\n\nThe following CLL/PLL slices are task-local instructions in the USER prompt. They do not override the standard Vegvisir system prompt, user authority, tool policy, secret boundary, approval policy, or safety boundaries. Use them for the exact current autonomy task.\n\nCLL CONTRACT SLICE:\n{}\n\nPLL PROMPT SLICE:\n{}\n\nRequired response: continue implementing/verifying the current node, write/update the JSON completion evidence packet when claiming node completion, update `{plan_path}` as items are completed, and only mark items `- [x]` when actually complete. Provide deliverables/evidence for completed work. The TUI controller will continue until every contract node has a checked checklist and validated evidence, or a deterministic stop condition occurs.",
            self.autonomy.objective,
            self.autonomy.step,
            self.autonomy.max_steps,
            self.autonomy.cll_path.as_deref().unwrap_or("-"),
            self.autonomy.pll_path.as_deref().unwrap_or("-"),
            self.autonomy.current_node_id.as_deref().unwrap_or("-"),
            self.autonomy.current_node_title.as_deref().unwrap_or("-"),
            self.autonomy.node_completed,
            self.autonomy.node_total,
            checklist.completed,
            checklist.total,
            checklist.unchecked,
            self.autonomy
                .current_evidence_path
                .as_deref()
                .unwrap_or("-"),
            self.autonomy
                .current_evidence_status
                .as_deref()
                .unwrap_or("-"),
            self.autonomy.current_evidence_valid,
            self.autonomy.current_evidence_blocked,
            self.autonomy.current_evidence_partial,
            self.autonomy.current_node_attempts,
            self.autonomy.max_node_attempts,
            if self.autonomy.current_evidence_errors.is_empty() {
                "-".to_string()
            } else {
                self.autonomy.current_evidence_errors.join(" | ")
            },
            slices.0,
            slices.1,
        )
    }

    fn compile_autonomy_libraries_if_possible(&mut self) {
        let Some(plan_path_text) = self.autonomy.plan_path.clone() else {
            return;
        };
        let plan_path = Path::new(&plan_path_text);
        let run_id = self.session.session_id.clone();
        match write_autonomy_libraries(&self.cwd, plan_path, &self.autonomy.objective, &run_id) {
            Ok(paths) => {
                let cll_path = paths.cll_path.display().to_string();
                let pll_path = paths.pll_path.display().to_string();
                let manifest_path = paths.manifest_path.display().to_string();
                let state_path = paths.state_path.display().to_string();
                let journal_path = paths.journal_path.display().to_string();
                let changed = self.autonomy.cll_path.as_deref() != Some(cll_path.as_str())
                    || self.autonomy.pll_path.as_deref() != Some(pll_path.as_str())
                    || self.autonomy.manifest_path.as_deref() != Some(manifest_path.as_str())
                    || self.autonomy.state_path.as_deref() != Some(state_path.as_str())
                    || self.autonomy.journal_path.as_deref() != Some(journal_path.as_str());
                self.autonomy.cll_path = Some(cll_path.clone());
                self.autonomy.pll_path = Some(pll_path.clone());
                self.autonomy.manifest_path = Some(manifest_path.clone());
                self.autonomy.state_path = Some(state_path.clone());
                self.autonomy.journal_path = Some(journal_path.clone());
                if changed {
                    self.push_system_message(format!(
                        "Autonomy plan compiled: CLL `{cll_path}`, PLL `{pll_path}`, manifest `{manifest_path}`, state `{state_path}`."
                    ));
                }
                self.logger.emit(
                    "autonomy_plan_compiled",
                    json!({
                        "session": self.session.session_id,
                        "workspace": self.cwd.display().to_string(),
                        "plan_path": plan_path_text,
                        "cll_path": cll_path,
                        "pll_path": pll_path,
                        "manifest_path": manifest_path,
                        "state_path": state_path,
                        "journal_path": journal_path,
                    }),
                );
            }
            Err(error) => {
                self.push_system_message(format!(
                    "Autonomy plan compile warning: failed to generate CLL/PLL from `{}`: {error}",
                    plan_path.display()
                ));
                self.logger.emit(
                    "autonomy_plan_compile_failed",
                    json!({
                        "session": self.session.session_id,
                        "workspace": self.cwd.display().to_string(),
                        "plan_path": plan_path_text,
                        "error": error.to_string(),
                    }),
                );
            }
        }
    }

    fn autonomy_current_library_slices(&self) -> (String, String) {
        let Some(plan_path_text) = self.autonomy.plan_path.as_deref() else {
            return (
                "No CLL slice available yet: plan path is unset.".to_string(),
                "No PLL slice available yet: plan path is unset.".to_string(),
            );
        };
        match current_autonomy_node_slices(
            &self.cwd,
            Path::new(plan_path_text),
            &self.autonomy.objective,
        ) {
            Ok(Some((_status, cll, pll))) => (cll, pll),
            Ok(None) => (
                "No CLL slice available yet: write the Markdown plan/checklist so Vegvisir can compile it.".to_string(),
                "No PLL slice available yet: write the Markdown plan/checklist so Vegvisir can compile it.".to_string(),
            ),
            Err(error) => (
                format!("Failed to read CLL slice: {error}"),
                format!("Failed to read PLL slice: {error}"),
            ),
        }
    }

    fn refresh_autonomy_node_status(&mut self) {
        let Some(plan_path_text) = self.autonomy.plan_path.as_deref() else {
            return;
        };
        match read_autonomy_plan_status(
            &self.cwd,
            Path::new(plan_path_text),
            &self.autonomy.objective,
        ) {
            Ok(Some(status)) => {
                let previous_node = self.autonomy.current_node_id.clone();
                let previous_evidence_valid = self.autonomy.current_evidence_valid;
                let previous_evidence_status = self.autonomy.current_evidence_status.clone();
                let current_node_id = status.current_node_id.clone();
                let current_node_title = status.current_node_title.clone();
                let current_evidence_path = status.current_evidence_path.clone();
                let current_evidence_errors = status.current_evidence_errors.clone();
                self.autonomy.node_total = status.total_nodes;
                self.autonomy.node_completed = status.completed_nodes;
                self.autonomy.current_node_id = current_node_id.clone();
                self.autonomy.current_node_title = current_node_title.clone();
                self.autonomy.current_evidence_path = current_evidence_path.clone();
                self.autonomy.current_evidence_valid = status.current_evidence_valid;
                self.autonomy.current_evidence_status = status.current_evidence_status.clone();
                self.autonomy.current_evidence_blocked = status.current_evidence_blocked;
                self.autonomy.current_evidence_partial = status.current_evidence_partial;
                self.autonomy.current_evidence_errors = current_evidence_errors.clone();
                self.autonomy.current_node_attempts =
                    self.autonomy_attempt_count_for_current_node();
                self.autonomy.journal_path = status
                    .journal_path
                    .clone()
                    .or(self.autonomy.journal_path.clone());
                if previous_node != current_node_id
                    || previous_evidence_valid != self.autonomy.current_evidence_valid
                    || previous_evidence_status != self.autonomy.current_evidence_status
                {
                    if let Some(journal_path) = self.autonomy.journal_path.as_deref() {
                        let _ = append_autonomy_journal_event(
                            &self.cwd,
                            Path::new(journal_path),
                            "node_status",
                            json!({
                                "session": self.session.session_id,
                                "current_node_id": current_node_id,
                                "current_node_title": current_node_title,
                                "node_total": status.total_nodes,
                                "node_completed": status.completed_nodes,
                                "current_evidence_path": current_evidence_path,
                                "current_evidence_status": self.autonomy.current_evidence_status,
                                "current_evidence_valid": self.autonomy.current_evidence_valid,
                                "current_evidence_blocked": self.autonomy.current_evidence_blocked,
                                "current_evidence_partial": self.autonomy.current_evidence_partial,
                                "current_node_attempts": self.autonomy.current_node_attempts,
                                "current_evidence_errors": current_evidence_errors,
                            }),
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.push_system_message(format!(
                    "Autonomy node status warning: failed to read `{plan_path_text}`: {error}"
                ));
            }
        }
    }

    fn autonomy_validate_command(&mut self, maybe_plan: Option<&String>) -> String {
        let Some(plan_path_text) = maybe_plan
            .map(String::as_str)
            .or(self.autonomy.plan_path.as_deref())
        else {
            return "Usage: /autonomy validate [plan-path] (or start/resume an autonomy run first)"
                .to_string();
        };
        let plan_path = Path::new(plan_path_text);
        match write_autonomy_libraries(
            &self.cwd,
            plan_path,
            &self.autonomy.objective,
            &self.session.session_id,
        )
        .and_then(|_| read_autonomy_plan_status(&self.cwd, plan_path, &self.autonomy.objective))
        {
            Ok(Some(status)) => {
                let paths = autonomy_library_paths_for_plan(plan_path);
                let summary = format!(
                    "Autonomy validation for `{}`\nnodes={}/{}\ncurrent_node={}\nevidence_status={}\nevidence_valid={}\nevidence_blocked={}\nevidence_partial={}\nevidence_errors={}\nstate={}\njournal={}",
                    plan_path_text,
                    status.completed_nodes,
                    status.total_nodes,
                    status.current_node_id.as_deref().unwrap_or("-"),
                    status.current_evidence_status.as_deref().unwrap_or("-"),
                    status.current_evidence_valid,
                    status.current_evidence_blocked,
                    status.current_evidence_partial,
                    if status.current_evidence_errors.is_empty() {
                        "-".to_string()
                    } else {
                        status.current_evidence_errors.join(" | ")
                    },
                    paths.state_path.display(),
                    paths.journal_path.display(),
                );
                let _ = append_autonomy_journal_event(
                    &self.cwd,
                    &paths.journal_path,
                    "manual_validate",
                    json!({
                        "session": self.session.session_id,
                        "plan_path": plan_path_text,
                        "node_total": status.total_nodes,
                        "node_completed": status.completed_nodes,
                        "current_node_id": status.current_node_id,
                        "current_evidence_valid": status.current_evidence_valid,
                        "current_evidence_status": status.current_evidence_status,
                        "current_evidence_blocked": status.current_evidence_blocked,
                        "current_evidence_partial": status.current_evidence_partial,
                        "current_evidence_errors": status.current_evidence_errors,
                    }),
                );
                summary
            }
            Ok(None) => {
                format!("Autonomy validation failed: plan `{plan_path_text}` was not found.")
            }
            Err(error) => format!("Autonomy validation failed for `{plan_path_text}`: {error}"),
        }
    }

    fn autonomy_resume_command(&mut self, maybe_plan: Option<&String>) -> String {
        let Some(plan_path_text) = maybe_plan else {
            return "Usage: /autonomy resume <plan-path>".to_string();
        };
        let plan_path = PathBuf::from(plan_path_text);
        match write_autonomy_libraries(
            &self.cwd,
            &plan_path,
            &self.autonomy.objective,
            &self.session.session_id,
        )
        .and_then(|paths| {
            let status =
                read_autonomy_plan_status(&self.cwd, &plan_path, &self.autonomy.objective)?
                    .ok_or_else(|| anyhow::anyhow!("plan was not found after compile"))?;
            Ok((paths, status))
        }) {
            Ok((paths, status)) => {
                self.autonomy.enabled = true;
                self.autonomy.active = true;
                if self.autonomy.objective.trim().is_empty() {
                    self.autonomy.objective = format!("Resume autonomy plan {plan_path_text}");
                }
                self.autonomy.plan_path = Some(plan_path_text.clone());
                self.autonomy.cll_path = Some(paths.cll_path.display().to_string());
                self.autonomy.pll_path = Some(paths.pll_path.display().to_string());
                self.autonomy.manifest_path = Some(paths.manifest_path.display().to_string());
                self.autonomy.state_path = Some(paths.state_path.display().to_string());
                self.autonomy.journal_path = Some(paths.journal_path.display().to_string());
                self.autonomy.node_total = status.total_nodes;
                self.autonomy.node_completed = status.completed_nodes;
                self.autonomy.current_node_id = status.current_node_id.clone();
                self.autonomy.current_node_title = status.current_node_title.clone();
                self.autonomy.current_evidence_path = status.current_evidence_path.clone();
                self.autonomy.current_evidence_valid = status.current_evidence_valid;
                self.autonomy.current_evidence_status = status.current_evidence_status.clone();
                self.autonomy.current_evidence_blocked = status.current_evidence_blocked;
                self.autonomy.current_evidence_partial = status.current_evidence_partial;
                self.autonomy.current_evidence_errors = status.current_evidence_errors.clone();
                self.autonomy.current_node_attempts =
                    self.autonomy_attempt_count_for_current_node();
                self.autonomy.step = self.autonomy.step.max(1);
                self.autonomy.last_status = format!(
                    "resumed; current node {} ({}/{})",
                    self.autonomy.current_node_id.as_deref().unwrap_or("-"),
                    self.autonomy.node_completed,
                    self.autonomy.node_total
                );
                let _ = append_autonomy_journal_event(
                    &self.cwd,
                    &paths.journal_path,
                    "run_resumed",
                    json!({
                        "session": self.session.session_id,
                        "plan_path": plan_path_text,
                        "current_node_id": self.autonomy.current_node_id,
                        "node_total": self.autonomy.node_total,
                        "node_completed": self.autonomy.node_completed,
                    }),
                );
                self.autonomy_status()
            }
            Err(error) => format!(
                "Autonomy resume failed for `{}`: {error}",
                plan_path.display()
            ),
        }
    }

    fn autonomy_attempt_count_for_current_node(&self) -> usize {
        let Some(journal_path) = self.autonomy.journal_path.as_deref() else {
            return 0;
        };
        let Some(current_node_id) = self.autonomy.current_node_id.as_deref() else {
            return 0;
        };
        let Ok(journal) = std::fs::read_to_string(self.cwd.join(journal_path)) else {
            return 0;
        };
        journal
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|entry| {
                entry.get("event").and_then(|event| event.as_str()) == Some("node_status")
            })
            .filter(|entry| {
                entry
                    .pointer("/payload/current_node_id")
                    .and_then(|node| node.as_str())
                    == Some(current_node_id)
            })
            .count()
    }

    fn autonomy_progress_signature_with_checklist(
        &self,
        checklist: &AutonomyChecklistStatus,
    ) -> String {
        let mut signature = self.autonomy_progress_signature();
        signature.push_str(&format!(
            ";plan={};current_node={};evidence_path={};evidence_status={};evidence_valid={};evidence_blocked={};evidence_partial={};node_attempts={}/{};nodes={}/{};checklist={}/{};unchecked={}",
            self.autonomy.plan_path.as_deref().unwrap_or("-"),
            self.autonomy.current_node_id.as_deref().unwrap_or("-"),
            self.autonomy
                .current_evidence_path
                .as_deref()
                .unwrap_or("-"),
            self.autonomy.current_evidence_status.as_deref().unwrap_or("-"),
            self.autonomy.current_evidence_valid,
            self.autonomy.current_evidence_blocked,
            self.autonomy.current_evidence_partial,
            self.autonomy.current_node_attempts,
            self.autonomy.max_node_attempts,
            self.autonomy.node_completed,
            self.autonomy.node_total,
            checklist.completed,
            checklist.total,
            checklist.unchecked
        ));
        signature
    }

    fn autonomy_progress_signature(&self) -> String {
        let message_count = self.session.messages.len();
        let tool_message_count = self
            .session
            .messages
            .iter()
            .filter(|message| message.role == "system" && is_live_tool_message(&message.content))
            .count();
        let approvals = self.tool_executor.guardrails.approvals.pending_len();
        let last_assistant_len = self
            .session
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "assistant")
            .map(|message| message.content.len())
            .unwrap_or_default();
        format!(
            "messages={message_count};tools={tool_message_count};approvals={approvals};assistant_len={last_assistant_len}"
        )
    }

    fn autonomy_plan_path_for_current_run(&self) -> String {
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
        format!("{AUTONOMY_PLAN_DIR}/{safe_session}-plan.md")
    }

    fn autonomy_checklist_status(&self) -> AutonomyChecklistStatus {
        let Some(plan_path) = self.autonomy.plan_path.as_deref() else {
            return AutonomyChecklistStatus {
                total: 0,
                completed: 0,
                unchecked: 0,
                has_checklist: false,
            };
        };
        let path = self.cwd.join(Path::new(plan_path));
        let Ok(content) = std::fs::read_to_string(path) else {
            return AutonomyChecklistStatus {
                total: 0,
                completed: 0,
                unchecked: 0,
                has_checklist: false,
            };
        };
        parse_markdown_checklist_status(&content)
    }
}

pub(crate) fn parse_markdown_checklist_status(content: &str) -> AutonomyChecklistStatus {
    let mut total = 0usize;
    let mut completed = 0usize;
    for line in content.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed
            .strip_prefix('-')
            .or_else(|| trimmed.strip_prefix('*'))
        else {
            continue;
        };
        let rest = rest.trim_start();
        if rest.len() < 3 || !rest.starts_with('[') {
            continue;
        }
        let Some(mark) = rest.chars().nth(1) else {
            continue;
        };
        if rest.chars().nth(2) != Some(']') {
            continue;
        }
        if matches!(mark, ' ' | 'x' | 'X') {
            total = total.saturating_add(1);
            if matches!(mark, 'x' | 'X') {
                completed = completed.saturating_add(1);
            }
        }
    }
    AutonomyChecklistStatus {
        total,
        completed,
        unchecked: total.saturating_sub(completed),
        has_checklist: total > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_checklist_status_counts_checked_and_unchecked_items() {
        let status = parse_markdown_checklist_status(
            "# Plan\n- [x] inspect\n- [ ] implement\n  * [X] verify\n- not a task\n",
        );
        assert!(status.has_checklist);
        assert_eq!(status.total, 3);
        assert_eq!(status.completed, 2);
        assert_eq!(status.unchecked, 1);
    }

    #[test]
    fn markdown_checklist_status_requires_task_items() {
        let status = parse_markdown_checklist_status("# Plan\nNo task list here.\n");
        assert!(!status.has_checklist);
        assert_eq!(status.total, 0);
        assert_eq!(status.completed, 0);
        assert_eq!(status.unchecked, 0);
    }
}
