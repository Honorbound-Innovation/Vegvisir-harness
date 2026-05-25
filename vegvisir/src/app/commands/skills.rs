use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::{
    core::load_skill_definitions,
    guardrails::ApprovalRequest,
    lsl::{
        LslPatchRequest, LslSkillDraft, LslSkillTrace, append_skill_trace, compile_lsl_roots,
        curate_registry, detect_missing_skill_candidates, forge_candidate_subskill,
        load_or_compile_lsl_roots, lsl_registry_status, patch_subskill, read_skill_traces,
        transition_subskill_status, update_skill_metrics_for_load,
    },
};

use super::super::lsl_runtime::redact_trace_query;
use super::super::{
    LslRuntimeConfig, TuiApplication, comma_items, list_or_dash, parse_config_value,
};

impl TuiApplication {
    pub(crate) fn skills_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok(self.skills());
        }
        match args[0].as_str() {
            "compile" => self.compile_skills_command(),
            "eval" => self.eval_skills_command(args.get(1).map(String::as_str)),
            "forge" => self.forge_skill_command(&args[1..]),
            "curate" => self.curate_skills_command(),
            "detect" => self.detect_missing_skills_command(),
            "patch" => self.patch_skill_command(&args[1..]),
            "trace" => self.skill_trace_command(),
            "config" => self.skills_config_command(&args[1..]),
            "explain" => self.explain_skills_command(&args[1..]),
            "invoke" => self.invoke_skill_command(&args[1..]),
            "promote" => self.set_skill_status_command(&args[1..], "active"),
            "evaluate" | "evaluated" => self.set_skill_status_command(&args[1..], "evaluated"),
            "sandbox" => self.set_skill_status_command(&args[1..], "sandboxed"),
            "archive" => self.set_skill_status_command(&args[1..], "archived"),
            "status" => self.skills_status_command(),
            "route" => {
                if args.len() < 2 {
                    return Ok("Usage: /skills route <query>".to_string());
                }
                self.route_skills_command(&args[1..].join(" "))
            }
            "load" => self.load_skills_command(&args[1..]),
            _ => Ok(self.skills()),
        }
    }

    fn skills(&self) -> String {
        self.session
            .enabled_skills
            .iter()
            .map(|skill| {
                let usrl = skill
                    .metadata
                    .get("usrl_contracts")
                    .and_then(Value::as_array)
                    .map(|contracts| {
                        contracts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                    })
                    .filter(|contracts| !contracts.is_empty())
                    .map(|contracts| format!(" [contracts: {}]", contracts.join(",")))
                    .unwrap_or_default();
                format!(
                    "{}: {} - {}{}",
                    skill.category, skill.name, skill.description, usrl
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn compile_skills_command(&mut self) -> anyhow::Result<String> {
        let compiled = compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        self.session.enabled_skills = load_skill_definitions(&self.cwd, &self.data_root)?;
        Ok(format!(
            "Compiled {} LSL libraries, {} sub-skills, {} links, {} policies, {} evals.\nArtifacts: {}",
            compiled.registry.libraries.len(),
            compiled.registry.subskills.len(),
            compiled.registry.links.len(),
            compiled.registry.policies.len(),
            compiled.registry.evals.len(),
            self.compiled_skill_root().display()
        ))
    }

    fn forge_skill_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok(
                "Usage: /skills forge <library.subskill> | <title> | <summary> | <body> [| tags=a,b]"
                    .to_string(),
            );
        }
        let raw = args.join(" ");
        let parts = raw
            .split('|')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.len() < 4 {
            return Ok(
                "Usage: /skills forge <library.subskill> | <title> | <summary> | <body> [| tags=a,b]"
                    .to_string(),
            );
        }
        let tags = parts
            .iter()
            .skip(4)
            .find_map(|part| part.strip_prefix("tags="))
            .map(comma_items)
            .unwrap_or_else(|| {
                parts[0]
                    .split('.')
                    .skip(1)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            });
        let path = forge_candidate_subskill(
            &self.cwd.join("skills"),
            LslSkillDraft {
                id: parts[0].to_string(),
                title: parts[1].to_string(),
                summary: parts[2].to_string(),
                body: parts[3].to_string(),
                provenance: format!(
                    "forged via /skills forge in session {}",
                    self.session.session_id
                ),
                tags,
            },
        )?;
        let compiled = compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        self.session.enabled_skills = load_skill_definitions(&self.cwd, &self.data_root)?;
        Ok(format!(
            "Forged candidate sub-skill {} in {}.\nCompiled {} LSL libraries. Run `/skills eval {}` then `/skills promote {}` when it passes.",
            parts[0],
            path.display(),
            compiled.registry.libraries.len(),
            parts[0],
            parts[0]
        ))
    }

    fn set_skill_status_command(
        &mut self,
        args: &[String],
        status: &str,
    ) -> anyhow::Result<String> {
        let Some(id) = args.first() else {
            return Ok(format!("Usage: /skills {status} <library.subskill>"));
        };
        let compiled = load_or_compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        let reports = compiled.registry.eval_hooks(Some(id));
        let direct_active_override = status == "active";
        if matches!(status, "evaluated" | "active") {
            if reports.is_empty() {
                return Ok(format!(
                    "No eval hooks found for {id}; refusing transition to {status}."
                ));
            }
            let failed = reports
                .iter()
                .filter(|report| !report.passed)
                .map(|report| format!("{} ({:.2})", report.eval_id, report.score))
                .collect::<Vec<_>>();
            if !failed.is_empty() {
                return Ok(format!(
                    "Transition to {status} blocked for {id}; failing evals: {}",
                    failed.join(", ")
                ));
            }
        }
        let path = match transition_subskill_status(
            &self.cwd.join("skills"),
            id,
            status,
            &reports,
            direct_active_override,
        ) {
            Ok(path) => path,
            Err(error) => return Ok(format!("Status change blocked for {id}: {error}")),
        };
        compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        self.session.enabled_skills = load_skill_definitions(&self.cwd, &self.data_root)?;
        Ok(format!(
            "Set {id} status to {status} in {}.",
            path.display()
        ))
    }

    fn skills_status_command(&self) -> anyhow::Result<String> {
        let status = lsl_registry_status(&self.skill_roots(), &self.compiled_skill_root())?;
        let mut lines = vec![
            format!("compiled_exists: {}", status.compiled_exists),
            format!("fresh: {}", status.fresh),
            format!("source_count: {}", status.source_count),
            format!("compiled_source_count: {}", status.compiled_source_count),
            format!("artifacts: {}", self.compiled_skill_root().display()),
            format!("automatic_loading: {}", self.lsl_runtime_config().mode),
            format!(
                "skill_token_budget: {}",
                self.lsl_runtime_config().token_budget
            ),
            format!(
                "max_primary_subskills: {}",
                self.lsl_runtime_config().max_primary_subskills
            ),
            format!(
                "max_total_subskills: {}",
                self.lsl_runtime_config().max_total_subskills
            ),
            format!(
                "max_dependency_depth: {}",
                self.lsl_runtime_config().max_dependency_depth
            ),
        ];
        if !status.stale_sources.is_empty() {
            lines.push(format!(
                "stale_sources: {}",
                status.stale_sources.join(", ")
            ));
        }
        if !status.missing_sources.is_empty() {
            lines.push(format!(
                "missing_sources: {}",
                status.missing_sources.join(", ")
            ));
        }
        if !status.extra_compiled_sources.is_empty() {
            lines.push(format!(
                "extra_compiled_sources: {}",
                status.extra_compiled_sources.join(", ")
            ));
        }
        Ok(lines.join("\n"))
    }

    fn route_skills_command(&self, query: &str) -> anyhow::Result<String> {
        let compiled = load_or_compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        if !compiled.registry.issues.is_empty() {
            return Ok(format!(
                "LSL registry has issues:\n{}",
                compiled.registry.issues.join("\n")
            ));
        }
        let candidates = compiled.registry.route_candidates(query, 8);
        let routed = candidates
            .iter()
            .filter(|candidate| !candidate.excluded)
            .map(|candidate| candidate.id.clone())
            .collect::<Vec<_>>();
        self.record_skill_trace("route", query, routed, 0)?;
        if candidates.is_empty() {
            return Ok("No LSL sub-skills matched.".to_string());
        }
        Ok(candidates
            .into_iter()
            .map(|candidate| {
                let entry = compiled
                    .registry
                    .subskills
                    .get(&candidate.id)
                    .expect("routed id exists");
                format!(
                    "{}: {} [score {}; {}; {}]",
                    candidate.id,
                    entry
                        .subskill
                        .summary
                        .as_deref()
                        .or(entry.subskill.title.as_deref())
                        .unwrap_or("No summary provided."),
                    candidate.score,
                    if candidate.excluded {
                        "excluded"
                    } else {
                        "eligible"
                    },
                    if candidate.signals.is_empty() {
                        candidate.reason
                    } else {
                        candidate.signals.join(",")
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn load_skills_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /skills load [--tokens N] <query-or-subskill>".to_string());
        }
        let mut tokens = 3500usize;
        let mut query_parts = Vec::new();
        let mut index = 0;
        while index < args.len() {
            if args[index] == "--tokens" {
                let Some(value) = args.get(index + 1) else {
                    return Ok("Usage: /skills load [--tokens N] <query-or-subskill>".to_string());
                };
                tokens = value.parse().unwrap_or(tokens);
                index += 2;
                continue;
            }
            query_parts.push(args[index].clone());
            index += 1;
        }
        if query_parts.is_empty() {
            return Ok("Usage: /skills load [--tokens N] <query-or-subskill>".to_string());
        }
        let query = query_parts.join(" ");
        let compiled = load_or_compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        if !compiled.registry.issues.is_empty() {
            return Ok(format!(
                "LSL registry has issues:\n{}",
                compiled.registry.issues.join("\n")
            ));
        }
        let selected = if compiled.registry.subskills.contains_key(&query) {
            vec![query.clone()]
        } else {
            compiled.registry.route(&query, 3)
        };
        if selected.is_empty() {
            return Ok("No LSL sub-skills matched.".to_string());
        }
        let context = compiled
            .registry
            .load_context_for_query(&selected, &query, tokens, 2);
        let approval_queued = self.enqueue_lsl_approval_if_needed(&query, &context);
        let _ = update_skill_metrics_for_load(&self.cwd.join("skills"), &context.selected, None);
        self.record_skill_trace(
            "load",
            &query,
            context
                .selected
                .iter()
                .map(|loaded| loaded.id.clone())
                .collect(),
            context.used_tokens,
        )?;
        let mut sections = vec![format!(
            "Loaded {} sub-skills. tokens used {}/{}.",
            context.selected.len(),
            context.used_tokens,
            context.available_tokens
        )];
        if approval_queued {
            sections.push("Approval required sub-skills were queued in /approvals.".to_string());
        }
        if !context.blocked.is_empty() {
            sections.push(format!(
                "Blocked/approval-required: {}",
                context
                    .blocked
                    .iter()
                    .map(|decision| format!("{} ({})", decision.id, decision.reason))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !context.excluded.is_empty() {
            sections.push(format!(
                "Excluded: {}",
                context
                    .excluded
                    .iter()
                    .map(|decision| format!("{} ({})", decision.id, decision.reason))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !context.not_loaded_relevant.is_empty() {
            sections.push(format!(
                "Not loaded: {}",
                context
                    .not_loaded_relevant
                    .iter()
                    .map(|decision| format!("{} ({})", decision.id, decision.reason))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        sections.extend(context.selected.into_iter().map(|loaded| {
            format!(
                "== {} [{}; ~{} tokens]\n{}",
                loaded.id, loaded.mode, loaded.token_estimate, loaded.text
            )
        }));
        Ok(sections.join("\n\n"))
    }

    fn explain_skills_command(&self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /skills explain <query-or-subskill>".to_string());
        }
        let query = args.join(" ");
        let compiled = load_or_compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        let selected = if compiled.registry.subskills.contains_key(&query) {
            vec![query.clone()]
        } else {
            compiled
                .registry
                .route(&query, self.lsl_runtime_config().max_primary_subskills)
        };
        let context = compiled.registry.load_context_for_query(
            &selected,
            &query,
            self.lsl_runtime_config().token_budget,
            self.lsl_runtime_config().max_dependency_depth,
        );
        Ok(serde_json::to_string_pretty(&json!({
            "query": query,
            "selected": context.selected.iter().map(|item| json!({"id": item.id, "mode": item.mode, "reason": item.reason, "tokens": item.token_estimate})).collect::<Vec<_>>(),
            "blocked": context.blocked,
            "excluded": context.excluded,
            "not_loaded": context.not_loaded_relevant,
            "policy": context.policy_decisions,
            "tokens": {"used": context.used_tokens, "available": context.available_tokens, "remaining": context.remaining_tokens}
        }))?)
    }

    fn invoke_skill_command(&self, args: &[String]) -> anyhow::Result<String> {
        let Some(id) = args.first() else {
            return Ok("Usage: /skills invoke <subskill-id> [json-input]".to_string());
        };
        let input = args.get(1).cloned().unwrap_or_else(|| "{}".to_string());
        let compiled = load_or_compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        let Some(entry) = compiled.registry.subskills.get(id) else {
            return Ok(format!("Unknown LSL sub-skill: {id}"));
        };
        let missing = entry
            .subskill
            .inputs
            .iter()
            .filter(|field| field.required && !input.contains(&format!("\"{}\"", field.name)))
            .map(|field| field.name.clone())
            .collect::<Vec<_>>();
        let mut sections = vec![
            format!("Callable sub-skill: {id}"),
            format!("Input: {input}"),
        ];
        if !missing.is_empty() {
            sections.push(format!("Missing required inputs: {}", missing.join(", ")));
        }
        sections.push(crate::lsl::materialize_subskill(&entry.subskill, "body"));
        Ok(sections.join("\n\n"))
    }

    pub(crate) fn skills_config_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty()
            || matches!(
                args.first().map(String::as_str),
                Some("show") | Some("status")
            )
        {
            let cfg = self.lsl_runtime_config();
            return Ok(format!(
                "LSL runtime config\nmode={}\ntoken_budget={}\nmax_primary_subskills={}\nmax_total_subskills={}\nmax_dependency_depth={}\nallow_extended={}\nsemantic_router={}",
                cfg.mode,
                cfg.token_budget,
                cfg.max_primary_subskills,
                cfg.max_total_subskills,
                cfg.max_dependency_depth,
                cfg.allow_extended,
                cfg.semantic_router
            ));
        }
        if args.first().map(String::as_str) != Some("set") || args.len() < 3 {
            return Ok("Usage: /skills config [show|set <key> <value>]".to_string());
        }
        let mut defaults = self.config.load().unwrap_or_default();
        let key = format!("lsl_{}", args[1]);
        let value = parse_config_value(&args[2]);
        defaults.insert(key.clone(), value);
        self.config.save(&defaults)?;
        Ok(format!("Set {key}."))
    }

    fn patch_skill_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let raw = args.join(" ");
        let parts = raw
            .split('|')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.len() < 4 {
            return Ok("Usage: /skills patch <id> | <operation> | <path> | <value>".to_string());
        }
        let path = patch_subskill(
            &self.cwd.join("skills"),
            LslPatchRequest {
                target: parts[0].to_string(),
                operation: parts[1].to_string(),
                path: parts[2].to_string(),
                value: parts[3].to_string(),
            },
        )?;
        compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        self.session.enabled_skills = load_skill_definitions(&self.cwd, &self.data_root)?;
        Ok(format!("Patched {} in {}.", parts[0], path.display()))
    }

    fn curate_skills_command(&self) -> anyhow::Result<String> {
        let compiled = load_or_compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        let traces = read_skill_traces(&self.skill_trace_path())?;
        let evals = compiled.registry.eval_hooks(None);
        let report = curate_registry(&compiled.registry, &traces, &evals);
        Ok(format!(
            "total_subskills: {}\nactive_subskills: {}\ncandidate_subskills: {}\nstale_subskills: {}\narchived_subskills: {}\nduplicate_summary_groups: {}\nfailing_evals: {}\nleast_used_subskills: {}\nmissing_skill_candidates: {}
recommendations: {}",
            report.total_subskills,
            report.active_subskills,
            list_or_dash(&report.candidate_subskills),
            list_or_dash(&report.stale_subskills),
            list_or_dash(&report.archived_subskills),
            if report.duplicate_summary_groups.is_empty() {
                "-".to_string()
            } else {
                report
                    .duplicate_summary_groups
                    .iter()
                    .map(|group| group.join(","))
                    .collect::<Vec<_>>()
                    .join("; ")
            },
            list_or_dash(&report.failing_evals),
            list_or_dash(&report.least_used_subskills),
            list_or_dash(&report.missing_skill_candidates),
            if report.recommendations.is_empty() {
                "-".to_string()
            } else {
                report.recommendations.iter().map(|rec| format!("{}:{} ({})", rec.kind, rec.target, rec.suggested_action)).collect::<Vec<_>>().join("; ")
            }
        ))
    }

    fn detect_missing_skills_command(&self) -> anyhow::Result<String> {
        let traces = read_skill_traces(&self.skill_trace_path())?;
        let candidates = detect_missing_skill_candidates(&traces);
        if candidates.is_empty() {
            Ok("No missing skill candidates detected.".to_string())
        } else {
            Ok(format!(
                "Missing skill candidates:\n{}",
                candidates.join("\n")
            ))
        }
    }

    fn skill_trace_command(&self) -> anyhow::Result<String> {
        let traces = read_skill_traces(&self.skill_trace_path())?;
        if traces.is_empty() {
            return Ok("No skill traces recorded.".to_string());
        }
        Ok(traces
            .iter()
            .rev()
            .take(10)
            .map(|trace| {
                format!(
                    "{} {} -> {} (~{} tokens)",
                    trace.event,
                    trace.query,
                    if trace.selected.is_empty() {
                        "-".to_string()
                    } else {
                        trace.selected.join(",")
                    },
                    trace.token_estimate
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn eval_skills_command(&self, target: Option<&str>) -> anyhow::Result<String> {
        let compiled = load_or_compile_lsl_roots(&self.skill_roots(), &self.compiled_skill_root())?;
        if !compiled.registry.issues.is_empty() {
            return Ok(format!(
                "LSL registry has issues:\n{}",
                compiled.registry.issues.join("\n")
            ));
        }
        let reports = compiled.registry.eval_hooks(target);
        if reports.is_empty() {
            return Ok("No LSL eval hooks matched.".to_string());
        }
        Ok(reports
            .into_iter()
            .map(|report| {
                let mut line = format!(
                    "{} -> {}: {} ({:.2})",
                    report.eval_id,
                    report.target,
                    if report.passed { "passed" } else { "failed" },
                    report.score
                );
                if !report.missing_expected.is_empty() {
                    line.push_str(&format!(
                        "\n  missing expected: {}",
                        report.missing_expected.join("; ")
                    ));
                }
                if !report.present_forbidden.is_empty() {
                    line.push_str(&format!(
                        "\n  present forbidden: {}",
                        report.present_forbidden.join("; ")
                    ));
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub(crate) fn lsl_runtime_config(&self) -> LslRuntimeConfig {
        let defaults = self.config.load().unwrap_or_default();
        LslRuntimeConfig {
            mode: defaults
                .get("lsl_mode")
                .and_then(Value::as_str)
                .unwrap_or("suggestions")
                .to_string(),
            token_budget: defaults
                .get("lsl_token_budget")
                .and_then(Value::as_u64)
                .unwrap_or(3500) as usize,
            max_primary_subskills: defaults
                .get("lsl_max_primary_subskills")
                .and_then(Value::as_u64)
                .unwrap_or(3) as usize,
            max_total_subskills: defaults
                .get("lsl_max_total_subskills")
                .and_then(Value::as_u64)
                .unwrap_or(8) as usize,
            max_dependency_depth: defaults
                .get("lsl_max_dependency_depth")
                .and_then(Value::as_u64)
                .unwrap_or(2) as usize,
            allow_extended: defaults
                .get("lsl_allow_extended")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            semantic_router: defaults
                .get("lsl_semantic_router")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    pub(crate) fn skill_roots(&self) -> Vec<PathBuf> {
        vec![
            self.cwd.join(".vegvisir").join("skills"),
            self.cwd.join("skills"),
            self.data_root.join("skills"),
        ]
    }

    pub(crate) fn compiled_skill_root(&self) -> PathBuf {
        self.cwd.join(".vegvisir").join("compiled")
    }

    pub(crate) fn skill_trace_path(&self) -> PathBuf {
        self.compiled_skill_root().join("skill_traces.json")
    }

    pub(crate) fn record_skill_trace(
        &self,
        event: &str,
        query: &str,
        selected: Vec<String>,
        token_estimate: usize,
    ) -> anyhow::Result<()> {
        append_skill_trace(
            &self.skill_trace_path(),
            LslSkillTrace {
                event: event.to_string(),
                query: query.to_string(),
                selected,
                token_estimate,
                created_at: chrono::Utc::now().to_rfc3339(),
                ..LslSkillTrace::default()
            },
        )
    }

    pub(crate) fn enqueue_lsl_approval_if_needed(
        &mut self,
        query: &str,
        context: &crate::lsl::LoadedSkillContext,
    ) -> bool {
        let mut queued = false;
        for decision in context
            .blocked
            .iter()
            .filter(|decision| decision.decision == "approval_required")
        {
            let mut args = Map::new();
            args.insert("subskill".to_string(), Value::String(decision.id.clone()));
            args.insert(
                "query".to_string(),
                Value::String(redact_trace_query(query)),
            );
            let id = lsl_approval_request_id(&decision.id, query);
            self.tool_executor
                .guardrails
                .approvals
                .enqueue(ApprovalRequest {
                    id,
                    reason: decision.reason.clone(),
                    tool_name: "lsl.load".to_string(),
                    args,
                    risk_label: "lsl-policy-approval".to_string(),
                });
            queued = true;
        }
        queued
    }
}

fn lsl_approval_request_id(subskill: &str, query: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "lsl.load".hash(&mut hasher);
    subskill.hash(&mut hasher);
    redact_trace_query(query).hash(&mut hasher);
    format!("apr_lsl_{:016x}", hasher.finish())
}
