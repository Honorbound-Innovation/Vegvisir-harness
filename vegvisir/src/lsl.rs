use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::parallelism::{ParallelismConfig, run_parallel_ordered};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LinkedSkillLibrary {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub status: Option<String>,
    pub risk: Option<String>,
    pub metadata: BTreeMap<String, Value>,
    pub load_policy: LslLoadPolicy,
    pub policies: BTreeMap<String, LslPolicy>,
    pub index: BTreeMap<String, LslIndexItem>,
    pub subskills: Vec<LslSubskill>,
    pub links: Vec<LslLink>,
    pub evals: Vec<LslEval>,
    pub source_hash: Option<String>,
    pub canonical_hash: Option<String>,
    pub semantic_hash: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslLoadPolicy {
    pub default_context_mode: Option<String>,
    pub max_primary_subskills: Option<usize>,
    pub max_total_subskills: Option<usize>,
    pub require_dependency_closure: Option<bool>,
    pub allow_extended_load: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslPolicy {
    pub id: String,
    pub inherits: Option<String>,
    pub allowed: Vec<String>,
    pub requires_approval: Vec<String>,
    pub forbidden: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslIndexItem {
    pub id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub risk: Option<String>,
    pub token_cost: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslSubskill {
    pub id: String,
    pub title: Option<String>,
    pub version: Option<String>,
    pub status: Option<String>,
    pub skill_type: Option<String>,
    pub risk: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub activation_positive: Vec<String>,
    pub activation_negative: Vec<String>,
    pub inputs: Vec<LslSignatureField>,
    pub outputs: Vec<LslSignatureField>,
    pub required_concepts: Vec<String>,
    pub required_tools: Vec<String>,
    pub policy: LslPolicy,
    pub context_budget: BTreeMap<String, u64>,
    pub load: BTreeMap<String, String>,
    pub forbidden: Vec<String>,
    pub verification: Vec<String>,
    pub failure_modes: Vec<String>,
    pub metrics: BTreeMap<String, Value>,
    pub eval_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslSignatureField {
    pub name: String,
    pub field_type: String,
    pub required: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslLink {
    pub id: String,
    pub from: String,
    pub to: String,
    pub relation: Option<String>,
    pub strength: Option<f64>,
    pub load_hint: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslEval {
    pub id: String,
    pub target: Option<String>,
    pub task: Option<String>,
    pub expected: Vec<String>,
    pub forbidden: Vec<String>,
    pub scoring: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslRegistry {
    pub libraries: BTreeMap<String, LinkedSkillLibrary>,
    pub subskills: BTreeMap<String, LslRegistryEntry>,
    pub links: Vec<LslLink>,
    pub policies: BTreeMap<String, LslPolicy>,
    pub evals: BTreeMap<String, LslEval>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LslRegistryEntry {
    pub library_id: String,
    pub subskill: LslSubskill,
    pub index: Option<LslIndexItem>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LoadedSkillContext {
    pub selected: Vec<LoadedSubskill>,
    pub available_tokens: usize,
    pub used_tokens: usize,
    pub remaining_tokens: usize,
    #[serde(default)]
    pub blocked: Vec<LslSelectionDecision>,
    #[serde(default)]
    pub excluded: Vec<LslSelectionDecision>,
    #[serde(default)]
    pub not_loaded_relevant: Vec<LslSelectionDecision>,
    #[serde(default)]
    pub policy_decisions: Vec<LslPolicyDecision>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadedSubskill {
    pub id: String,
    pub mode: String,
    pub reason: String,
    pub token_estimate: usize,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslSelectionDecision {
    pub id: String,
    pub relation: Option<String>,
    pub decision: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslPolicyDecision {
    pub id: String,
    pub allowed: bool,
    pub approval_required: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslRouteCandidate {
    pub id: String,
    pub score: usize,
    #[serde(default)]
    pub lexical_score: usize,
    #[serde(default)]
    pub semantic_score: f64,
    pub signals: Vec<String>,
    pub excluded: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CallableSubskill {
    pub id: String,
    pub title: Option<String>,
    pub version: Option<String>,
    pub risk: Option<String>,
    pub inputs: Vec<LslSignatureField>,
    pub outputs: Vec<LslSignatureField>,
    pub required_tools: Vec<String>,
    pub materialization_modes: Vec<String>,
    pub policy: LslPolicy,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CallableInputValidation {
    pub callable_id: String,
    pub valid: bool,
    pub missing_required_inputs: Vec<String>,
    pub unknown_inputs: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CompiledLslRegistry {
    pub registry: LslRegistry,
    pub hashes: BTreeMap<String, LslHashes>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslHashes {
    #[serde(default)]
    pub source_path: String,
    pub source_hash: String,
    pub canonical_hash: String,
    pub semantic_hash: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslRegistryStatus {
    pub compiled_exists: bool,
    pub fresh: bool,
    pub source_count: usize,
    pub compiled_source_count: usize,
    pub stale_sources: Vec<String>,
    pub missing_sources: Vec<String>,
    pub extra_compiled_sources: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslEvalReport {
    pub eval_id: String,
    pub target: String,
    pub passed: bool,
    pub score: f64,
    pub missing_expected: Vec<String>,
    pub present_forbidden: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LslSkillDraft {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub provenance: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslSkillTrace {
    pub event: String,
    pub query: String,
    pub selected: Vec<String>,
    pub token_estimate: usize,
    #[serde(default)]
    pub available_tokens: usize,
    #[serde(default)]
    pub remaining_tokens: usize,
    #[serde(default)]
    pub load_modes: BTreeMap<String, String>,
    #[serde(default)]
    pub policy_decisions: Vec<LslPolicyDecision>,
    #[serde(default)]
    pub blocked: Vec<LslSelectionDecision>,
    #[serde(default)]
    pub excluded: Vec<LslSelectionDecision>,
    #[serde(default)]
    pub not_loaded_relevant: Vec<LslSelectionDecision>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslCuratorReport {
    pub total_subskills: usize,
    pub active_subskills: usize,
    pub candidate_subskills: Vec<String>,
    pub stale_subskills: Vec<String>,
    pub archived_subskills: Vec<String>,
    pub duplicate_summary_groups: Vec<Vec<String>>,
    pub failing_evals: Vec<String>,
    pub least_used_subskills: Vec<String>,
    pub missing_skill_candidates: Vec<String>,
    #[serde(default)]
    pub recommendations: Vec<LslCuratorRecommendation>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslCuratorRecommendation {
    pub kind: String,
    pub target: String,
    pub reason: String,
    pub suggested_action: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LslPatchRequest {
    pub target: String,
    pub operation: String,
    pub path: String,
    pub value: String,
}

#[derive(Clone, Debug)]
struct NamedBlock {
    keyword: String,
    name: Option<String>,
    body: String,
}

pub fn parse_lsl(source: &str) -> anyhow::Result<LinkedSkillLibrary> {
    let library = find_first_block(source, "library")?.context("missing library block")?;
    let library_id = library
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .context("library declaration must include an identifier")?;
    let blocks = top_level_blocks(&library.body)?;

    let mut parsed = LinkedSkillLibrary {
        id: library_id.clone(),
        name: library_id,
        ..LinkedSkillLibrary::default()
    };

    if let Some(meta) = blocks.iter().find(|block| block.keyword == "meta") {
        parsed.metadata = parse_field_map(&meta.body);
        parsed.name = string_field(&meta.body, "name")
            .or_else(|| string_field(&meta.body, "display_name"))
            .unwrap_or_else(|| parsed.id.clone());
        parsed.version = string_field(&meta.body, "version");
        parsed.status = atom_field(&meta.body, "status");
        parsed.risk = atom_field(&meta.body, "risk");
    }

    if let Some(load_policy) = blocks.iter().find(|block| block.keyword == "load_policy") {
        parsed.load_policy = LslLoadPolicy {
            default_context_mode: atom_field(&load_policy.body, "default_context_mode"),
            max_primary_subskills: usize_field(&load_policy.body, "max_primary_subskills"),
            max_total_subskills: usize_field(&load_policy.body, "max_total_subskills"),
            require_dependency_closure: bool_field(&load_policy.body, "require_dependency_closure"),
            allow_extended_load: atom_field(&load_policy.body, "allow_extended_load"),
        };
    }

    for policy in blocks.iter().filter(|block| block.keyword == "policy") {
        let id = policy
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .context("policy must include an identifier")?;
        parsed.policies.insert(
            id.clone(),
            LslPolicy {
                id,
                inherits: atom_field(&policy.body, "inherits"),
                allowed: list_field(&policy.body, "allowed"),
                requires_approval: list_field(&policy.body, "requires_approval"),
                forbidden: list_field(&policy.body, "forbidden"),
            },
        );
    }

    if let Some(index) = blocks.iter().find(|block| block.keyword == "index") {
        for item in named_blocks(&index.body, "item")? {
            let id = item
                .name
                .clone()
                .filter(|name| !name.is_empty())
                .context("index item must include an identifier")?;
            parsed.index.insert(
                id.clone(),
                LslIndexItem {
                    id,
                    title: string_field(&item.body, "title"),
                    summary: string_field(&item.body, "summary"),
                    tags: list_field(&item.body, "tags"),
                    risk: atom_field(&item.body, "risk"),
                    token_cost: numeric_block(&item.body, "token_cost")?,
                },
            );
        }
    }

    for block in blocks.iter().filter(|block| block.keyword == "subskill") {
        let id = block
            .name
            .clone()
            .or_else(|| atom_field(&block.body, "id"))
            .filter(|name| !name.is_empty())
            .context("subskill must include an identifier")?;
        parsed.subskills.push(LslSubskill {
            id,
            title: string_field(&block.body, "title"),
            version: string_field(&block.body, "version"),
            status: atom_field(&block.body, "status"),
            skill_type: atom_field(&block.body, "type"),
            risk: atom_field(&block.body, "risk"),
            summary: string_field(&block.body, "summary"),
            tags: list_field(&block.body, "tags"),
            activation_positive: nested_list_field(&block.body, "activation", "positive")?,
            activation_negative: nested_list_field(&block.body, "activation", "negative")?,
            inputs: signature_fields(&block.body, "input")?,
            outputs: signature_fields(&block.body, "output")?,
            required_concepts: nested_list_field(&block.body, "requires", "concepts")?,
            required_tools: nested_list_field(&block.body, "requires", "tools")?,
            policy: nested_policy(&block.body)?,
            context_budget: numeric_block(&block.body, "context_budget")?,
            load: text_block(&block.body, "load")?,
            forbidden: list_field(&block.body, "forbidden"),
            verification: list_field(&block.body, "verification"),
            failure_modes: list_field(&block.body, "failure_modes"),
            metrics: nested_value_map(&block.body, "metrics")?,
            eval_refs: list_field(&block.body, "eval_refs"),
        });
    }

    for block in blocks.iter().filter(|block| block.keyword == "link") {
        let id = block
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .context("link must include an identifier")?;
        parsed.links.push(LslLink {
            id,
            from: atom_field(&block.body, "from").unwrap_or_default(),
            to: atom_field(&block.body, "to").unwrap_or_default(),
            relation: atom_field(&block.body, "relation"),
            strength: number_field(&block.body, "strength"),
            load_hint: atom_field(&block.body, "load_hint"),
        });
    }

    for block in blocks.iter().filter(|block| block.keyword == "eval") {
        let id = block
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .context("eval must include an identifier")?;
        parsed.evals.push(LslEval {
            id,
            target: atom_field(&block.body, "target"),
            task: string_field(&block.body, "task"),
            expected: list_field(&block.body, "expected"),
            forbidden: list_field(&block.body, "forbidden"),
            scoring: float_block(&block.body, "scoring")?,
        });
    }

    parsed.source_hash = Some(sha256_hex(source.as_bytes()));
    parsed.canonical_hash = Some(sha256_hex(canonical_lsl_text(&parsed)?.as_bytes()));
    parsed.semantic_hash = Some(sha256_hex(semantic_lsl_text(&parsed)?.as_bytes()));

    validate_lsl(&parsed)?;
    Ok(parsed)
}

impl LslRegistry {
    pub fn from_libraries(libraries: Vec<LinkedSkillLibrary>) -> Self {
        let mut registry = Self::default();
        for library in libraries {
            for subskill in &library.subskills {
                if registry.subskills.contains_key(&subskill.id) {
                    registry
                        .issues
                        .push(format!("duplicate subskill id '{}'", subskill.id));
                    continue;
                }
                registry.subskills.insert(
                    subskill.id.clone(),
                    LslRegistryEntry {
                        library_id: library.id.clone(),
                        subskill: subskill.clone(),
                        index: library.index.get(&subskill.id).cloned(),
                    },
                );
            }
            registry.links.extend(library.links.clone());
            registry.policies.extend(library.policies.clone());
            registry.evals.extend(
                library
                    .evals
                    .iter()
                    .map(|eval| (eval.id.clone(), eval.clone())),
            );
            registry.libraries.insert(library.id.clone(), library);
        }
        registry.validate_references();
        registry
    }

    pub fn route(&self, query: &str, limit: usize) -> Vec<String> {
        self.route_candidates(query, limit)
            .into_iter()
            .filter(|candidate| !candidate.excluded)
            .map(|candidate| candidate.id)
            .collect()
    }

    pub fn route_candidates(&self, query: &str, limit: usize) -> Vec<LslRouteCandidate> {
        let query_terms = terms(query);
        let mut candidates = self
            .subskills
            .iter()
            .filter_map(|(id, entry)| {
                let mut signals = Vec::new();
                let lexical = route_score(&query_terms, entry);
                if lexical > 0 {
                    signals.push(format!("lexical:{lexical}"));
                }
                let activation_positive =
                    term_overlap(&query_terms, &entry.subskill.activation_positive);
                if activation_positive > 0 {
                    signals.push(format!("activation_positive:{activation_positive}"));
                }
                let activation_negative =
                    term_overlap(&query_terms, &entry.subskill.activation_negative);
                let status = entry.subskill.status.as_deref().unwrap_or("active");
                let status_penalty =
                    matches!(status, "candidate" | "sandboxed" | "stale" | "archived")
                        .then_some(format!("status:{status}"));
                let semantic = if lexical > 0 || activation_positive > 0 {
                    semantic_route_score(query, entry)
                } else {
                    0.0
                };
                if semantic > 0.0 {
                    signals.push(format!("semantic:{semantic:.3}"));
                }
                let has_route_match = lexical > 0 || activation_positive > 0 || semantic > 0.0;
                let tool_score = if has_route_match {
                    tool_compatibility_score(&entry.subskill.required_tools)
                } else {
                    0
                };
                if tool_score > 0 {
                    signals.push(format!("tool_compatible:{tool_score}"));
                }
                let success = metric_u64(&entry.subskill.metrics, "success_count");
                let failures = metric_u64(&entry.subskill.metrics, "failure_count");
                if success > 0 || failures > 0 {
                    signals.push(format!("runtime_success:{success}/{failures}"));
                }
                let risk_penalty = risk_penalty(entry.subskill.risk.as_deref());
                if risk_penalty > 0 {
                    signals.push(format!("risk_penalty:{risk_penalty}"));
                }
                let semantic_points = (semantic * 10.0).round() as usize;
                let success_points = if success + failures > 0 {
                    ((success as f64 / (success + failures) as f64) * 3.0).round() as usize
                } else {
                    0
                };
                let score = lexical
                    + (activation_positive * 2)
                    + semantic_points
                    + tool_score
                    + success_points
                    - risk_penalty.min(
                        lexical
                            + (activation_positive * 2)
                            + semantic_points
                            + tool_score
                            + success_points,
                    );
                if score == 0 && activation_negative == 0 {
                    return None;
                }
                let excluded = activation_negative > 0
                    || matches!(status, "archived" | "candidate" | "sandboxed");
                let mut reason = if activation_negative > 0 {
                    format!("negative activation matched ({activation_negative})")
                } else if matches!(status, "archived" | "candidate" | "sandboxed") {
                    format!("status {status} is not eligible for automatic routing")
                } else {
                    "eligible route candidate".to_string()
                };
                if let Some(penalty) = status_penalty {
                    signals.push(penalty);
                }
                if excluded && score == 0 {
                    reason = "excluded by route filters".to_string();
                }
                Some(LslRouteCandidate {
                    id: id.clone(),
                    score,
                    lexical_score: lexical,
                    semantic_score: semantic,
                    signals,
                    excluded,
                    reason,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.excluded
                .cmp(&right.excluded)
                .then_with(|| right.score.cmp(&left.score))
                .then_with(|| left.id.cmp(&right.id))
        });
        candidates.into_iter().take(limit).collect()
    }

    pub fn dependency_closure(&self, selected: &[String], max_depth: usize) -> Vec<String> {
        let mut ordered = Vec::new();
        let mut seen = BTreeSet::new();
        for id in selected {
            self.collect_dependencies(id, 0, max_depth, &mut seen, &mut ordered);
        }
        ordered
    }

    pub fn load_context(
        &self,
        selected: &[String],
        available_tokens: usize,
        max_depth: usize,
    ) -> LoadedSkillContext {
        self.load_context_for_query(selected, "", available_tokens, max_depth)
    }

    pub fn load_context_for_query(
        &self,
        selected: &[String],
        query: &str,
        available_tokens: usize,
        max_depth: usize,
    ) -> LoadedSkillContext {
        let mut used_tokens = 0usize;
        let mut loaded = Vec::new();
        let mut blocked = Vec::new();
        let mut excluded = Vec::new();
        let mut not_loaded_relevant = Vec::new();
        let mut policy_decisions = Vec::new();
        let selected_set = selected.iter().cloned().collect::<BTreeSet<_>>();
        let plan = self.resolve_dependency_plan(selected, query, max_depth);
        let max_total = self.max_total_subskills(selected).unwrap_or(usize::MAX);
        for decision in plan {
            if decision.decision == "exclude" {
                excluded.push(decision);
                continue;
            }
            let Some(entry) = self.subskills.get(&decision.id) else {
                not_loaded_relevant.push(LslSelectionDecision {
                    decision: "missing".to_string(),
                    reason: "sub-skill not found".to_string(),
                    ..decision
                });
                continue;
            };
            let policy = self.policy_decision(&decision.id, query);
            policy_decisions.push(policy.clone());
            if !policy.allowed {
                blocked.push(LslSelectionDecision {
                    decision: "blocked".to_string(),
                    reason: policy.reason,
                    ..decision
                });
                continue;
            }
            if policy.approval_required {
                blocked.push(LslSelectionDecision {
                    decision: "approval_required".to_string(),
                    reason: policy.reason,
                    ..decision
                });
                continue;
            }
            if loaded.len() >= max_total {
                not_loaded_relevant.push(LslSelectionDecision {
                    decision: "max_total_skipped".to_string(),
                    reason: format!("max total sub-skills reached ({max_total})"),
                    ..decision
                });
                continue;
            }
            let preferred_mode = if selected_set.contains(&decision.id) {
                "body"
            } else {
                self.load_hint_for(selected, &decision.id).unwrap_or("card")
            };
            let preferred_mode = if preferred_mode == "none" {
                "card"
            } else {
                preferred_mode
            };
            let (mode, text, token_estimate) = budgeted_materialization(
                &entry.subskill,
                preferred_mode,
                available_tokens.saturating_sub(used_tokens),
                loaded.is_empty(),
            );
            if used_tokens + token_estimate > available_tokens && !loaded.is_empty() {
                not_loaded_relevant.push(LslSelectionDecision {
                    decision: "budget_skipped".to_string(),
                    reason: format!("would exceed skill token budget ({used_tokens}+{token_estimate}>{available_tokens})"),
                    ..decision
                });
                continue;
            }
            used_tokens += token_estimate;
            loaded.push(LoadedSubskill {
                id: decision.id.clone(),
                mode: mode.to_string(),
                reason: decision.reason,
                token_estimate,
                text,
            });
        }
        LoadedSkillContext {
            selected: loaded,
            available_tokens,
            used_tokens,
            remaining_tokens: available_tokens.saturating_sub(used_tokens),
            blocked,
            excluded,
            not_loaded_relevant,
            policy_decisions,
        }
    }

    pub fn policy_decision(&self, id: &str, query: &str) -> LslPolicyDecision {
        let Some(entry) = self.subskills.get(id) else {
            return LslPolicyDecision {
                id: id.to_string(),
                allowed: false,
                approval_required: false,
                reason: "missing sub-skill".to_string(),
            };
        };
        let query_terms = terms(query);
        if term_overlap(&query_terms, &entry.subskill.activation_negative) > 0 {
            return LslPolicyDecision {
                id: id.to_string(),
                allowed: false,
                approval_required: false,
                reason: "negative activation matched task".to_string(),
            };
        }
        let effective = self
            .libraries
            .get(&entry.library_id)
            .and_then(|library| effective_subskill_policy(library, &entry.subskill).ok())
            .unwrap_or_else(|| entry.subskill.policy.clone());
        if term_overlap(&query_terms, &effective.forbidden) > 0
            || term_overlap(&query_terms, &entry.subskill.forbidden) > 0
        {
            return LslPolicyDecision {
                id: id.to_string(),
                allowed: false,
                approval_required: false,
                reason: "task matched forbidden policy or sub-skill behavior".to_string(),
            };
        }
        if term_overlap(&query_terms, &effective.requires_approval) > 0 {
            return LslPolicyDecision {
                id: id.to_string(),
                allowed: true,
                approval_required: true,
                reason: "task matched approval-required policy".to_string(),
            };
        }
        LslPolicyDecision {
            id: id.to_string(),
            allowed: true,
            approval_required: false,
            reason: "policy allowed".to_string(),
        }
    }

    pub fn resolve_dependency_plan(
        &self,
        selected: &[String],
        query: &str,
        max_depth: usize,
    ) -> Vec<LslSelectionDecision> {
        let mut ordered = Vec::new();
        let mut seen = BTreeSet::new();
        let query_terms = terms(query);
        for id in selected {
            let primary_id = self.preferred_replacement(id).unwrap_or(id);
            if primary_id != id {
                ordered.push(LslSelectionDecision {
                    id: id.clone(),
                    relation: Some("replaces".to_string()),
                    decision: "exclude".to_string(),
                    reason: format!("replaced by {primary_id}"),
                });
            }
            let policy = self.policy_decision(primary_id, query);
            if !policy.allowed {
                let fallbacks = self.fallback_targets(primary_id);
                if fallbacks.is_empty() {
                    self.collect_dependency_plan(
                        primary_id,
                        None,
                        0,
                        max_depth,
                        &query_terms,
                        &mut seen,
                        &mut ordered,
                        "primary routed sub-skill".to_string(),
                    );
                } else {
                    ordered.push(LslSelectionDecision {
                        id: primary_id.to_string(),
                        relation: None,
                        decision: "exclude".to_string(),
                        reason: format!(
                            "primary sub-skill blocked by policy; trying fallback: {}",
                            policy.reason
                        ),
                    });
                    for fallback in fallbacks {
                        self.collect_dependency_plan(
                            &fallback,
                            Some("fallback".to_string()),
                            0,
                            max_depth,
                            &query_terms,
                            &mut seen,
                            &mut ordered,
                            format!("fallback for blocked {primary_id}"),
                        );
                    }
                }
                continue;
            }
            self.collect_dependency_plan(
                primary_id,
                None,
                0,
                max_depth,
                &query_terms,
                &mut seen,
                &mut ordered,
                if primary_id == id {
                    "primary routed sub-skill".to_string()
                } else {
                    format!("replacement selected for {id}")
                },
            );
        }
        ordered
    }

    fn preferred_replacement<'a>(&'a self, id: &'a str) -> Option<&'a str> {
        self.links
            .iter()
            .find(|link| {
                link.relation.as_deref() == Some("replaces")
                    && link.to == id
                    && self.subskills.get(&link.from).is_some_and(|entry| {
                        matches!(
                            entry.subskill.status.as_deref().unwrap_or("active"),
                            "active" | "evaluated" | "pinned"
                        )
                    })
            })
            .map(|link| link.from.as_str())
    }

    fn fallback_targets(&self, id: &str) -> Vec<String> {
        self.links
            .iter()
            .filter(|link| link.from == id && link.relation.as_deref() == Some("fallback"))
            .filter(|link| self.subskills.contains_key(&link.to))
            .map(|link| link.to.clone())
            .collect()
    }

    fn validate_references(&mut self) {
        for link in &self.links {
            if !self.subskills.contains_key(&link.from) {
                self.issues.push(format!(
                    "link '{}' references unknown source '{}'",
                    link.id, link.from
                ));
            }
            if !self.subskills.contains_key(&link.to) {
                self.issues.push(format!(
                    "link '{}' references unknown target '{}'",
                    link.id, link.to
                ));
            }
        }
        for (id, entry) in &self.subskills {
            for required in &entry.subskill.required_concepts {
                if !self.subskills.contains_key(required) {
                    self.issues.push(format!(
                        "subskill '{}' requires unknown concept '{}'",
                        id, required
                    ));
                }
            }
        }
    }

    fn max_total_subskills(&self, selected: &[String]) -> Option<usize> {
        selected
            .iter()
            .filter_map(|id| self.subskills.get(id))
            .filter_map(|entry| self.libraries.get(&entry.library_id))
            .filter_map(|library| library.load_policy.max_total_subskills)
            .min()
    }

    fn collect_dependency_plan(
        &self,
        id: &str,
        relation: Option<String>,
        depth: usize,
        max_depth: usize,
        query_terms: &BTreeSet<String>,
        seen: &mut BTreeSet<String>,
        ordered: &mut Vec<LslSelectionDecision>,
        reason: String,
    ) {
        if !seen.insert(id.to_string()) {
            return;
        }
        ordered.push(LslSelectionDecision {
            id: id.to_string(),
            relation: relation.clone(),
            decision: "load".to_string(),
            reason,
        });
        if depth >= max_depth {
            return;
        }
        let Some(entry) = self.subskills.get(id) else {
            return;
        };
        for required in &entry.subskill.required_concepts {
            if self.subskills.contains_key(required) {
                self.collect_dependency_plan(
                    required,
                    Some("requires".to_string()),
                    depth + 1,
                    max_depth,
                    query_terms,
                    seen,
                    ordered,
                    "required concept dependency".to_string(),
                );
            }
        }
        for link in self.links.iter().filter(|link| link.from == id) {
            let relation = link.relation.as_deref().unwrap_or("related");
            match relation {
                "requires" | "extends" | "specializes" | "generalizes" => {
                    self.collect_dependency_plan(
                        &link.to,
                        Some(relation.to_string()),
                        depth + 1,
                        max_depth,
                        query_terms,
                        seen,
                        ordered,
                        format!("{relation} dependency via {}", link.id),
                    );
                }
                "related" => {
                    if depth + 1 <= max_depth && !seen.contains(&link.to) {
                        self.collect_dependency_plan(
                            &link.to,
                            Some(relation.to_string()),
                            depth + 1,
                            max_depth,
                            query_terms,
                            seen,
                            ordered,
                            format!("related optional context via {}", link.id),
                        );
                    }
                }
                "conflicts" => {
                    if !seen.contains(&link.to) {
                        ordered.push(LslSelectionDecision {
                            id: link.to.clone(),
                            relation: Some(relation.to_string()),
                            decision: "exclude".to_string(),
                            reason: format!("conflicts with selected sub-skill via {}", link.id),
                        });
                    }
                }
                "replaces" => {
                    if !seen.contains(&link.to) {
                        ordered.push(LslSelectionDecision {
                            id: link.to.clone(),
                            relation: Some(relation.to_string()),
                            decision: "exclude".to_string(),
                            reason: format!("replaced by {} via {}", id, link.id),
                        });
                    }
                }
                "fallback" => {
                    if term_overlap(query_terms, &[link.to.clone()]) > 0 {
                        self.collect_dependency_plan(
                            &link.to,
                            Some(relation.to_string()),
                            depth + 1,
                            max_depth,
                            query_terms,
                            seen,
                            ordered,
                            format!("fallback context via {}", link.id),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_dependencies(
        &self,
        id: &str,
        depth: usize,
        max_depth: usize,
        seen: &mut BTreeSet<String>,
        ordered: &mut Vec<String>,
    ) {
        if !seen.insert(id.to_string()) {
            return;
        }
        ordered.push(id.to_string());
        if depth >= max_depth {
            return;
        }
        let Some(entry) = self.subskills.get(id) else {
            return;
        };
        for required in &entry.subskill.required_concepts {
            if self.subskills.contains_key(required) {
                self.collect_dependencies(required, depth + 1, max_depth, seen, ordered);
            }
        }
        for link in self
            .links
            .iter()
            .filter(|link| link.from == id && link.relation.as_deref() == Some("requires"))
        {
            self.collect_dependencies(&link.to, depth + 1, max_depth, seen, ordered);
        }
    }

    fn load_hint_for<'a>(&'a self, selected: &[String], dependency: &str) -> Option<&'a str> {
        self.links
            .iter()
            .find(|link| {
                selected.iter().any(|selected| selected == &link.from) && link.to == dependency
            })
            .and_then(|link| link.load_hint.as_deref())
    }

    pub fn callable_subskill(&self, id: &str) -> Option<CallableSubskill> {
        let entry = self.subskills.get(id)?;
        let policy = self
            .libraries
            .get(&entry.library_id)
            .and_then(|library| effective_subskill_policy(library, &entry.subskill).ok())
            .unwrap_or_else(|| entry.subskill.policy.clone());
        let mut materialization_modes = entry.subskill.load.keys().cloned().collect::<Vec<_>>();
        materialization_modes.sort();
        Some(CallableSubskill {
            id: id.to_string(),
            title: entry.subskill.title.clone(),
            version: entry.subskill.version.clone(),
            risk: entry.subskill.risk.clone(),
            inputs: entry.subskill.inputs.clone(),
            outputs: entry.subskill.outputs.clone(),
            required_tools: entry.subskill.required_tools.clone(),
            materialization_modes,
            policy,
        })
    }

    pub fn validate_callable_inputs(&self, id: &str, input: &Value) -> CallableInputValidation {
        let Some(callable) = self.callable_subskill(id) else {
            return CallableInputValidation {
                callable_id: id.to_string(),
                valid: false,
                warnings: vec!["unknown callable sub-skill".to_string()],
                ..CallableInputValidation::default()
            };
        };
        let object = input.as_object();
        let mut missing_required_inputs = Vec::new();
        let mut unknown_inputs = Vec::new();
        let mut warnings = Vec::new();
        for field in &callable.inputs {
            if field.required && !object.is_some_and(|obj| obj.contains_key(&field.name)) {
                missing_required_inputs.push(field.name.clone());
            }
        }
        if let Some(obj) = object {
            for key in obj.keys() {
                if !callable.inputs.iter().any(|field| &field.name == key) {
                    unknown_inputs.push(key.clone());
                }
            }
        } else if !callable.inputs.is_empty() {
            warnings
                .push("input should be a JSON object keyed by signature field name".to_string());
        }
        let valid = missing_required_inputs.is_empty() && object.is_some();
        CallableInputValidation {
            callable_id: id.to_string(),
            valid,
            missing_required_inputs,
            unknown_inputs,
            warnings,
        }
    }

    pub fn eval_hooks(&self, target: Option<&str>) -> Vec<LslEvalReport> {
        let mut reports = Vec::new();
        for eval in self.evals.values() {
            let Some(eval_target) = eval.target.as_deref() else {
                continue;
            };
            if target.is_some_and(|target| target != eval_target && target != eval.id) {
                continue;
            }
            let Some(entry) = self.subskills.get(eval_target) else {
                reports.push(LslEvalReport {
                    eval_id: eval.id.clone(),
                    target: eval_target.to_string(),
                    passed: false,
                    score: 0.0,
                    missing_expected: vec![format!("missing target subskill {eval_target}")],
                    present_forbidden: Vec::new(),
                });
                continue;
            };
            let context = materialize_subskill(&entry.subskill, "extended").to_ascii_lowercase();
            let missing_expected = eval
                .expected
                .iter()
                .filter(|expected| !context.contains(&expected.to_ascii_lowercase()))
                .cloned()
                .collect::<Vec<_>>();
            let present_forbidden = eval
                .forbidden
                .iter()
                .filter(|forbidden| context.contains(&forbidden.to_ascii_lowercase()))
                .cloned()
                .collect::<Vec<_>>();
            let score = eval_score(eval, &missing_expected, &present_forbidden);
            reports.push(LslEvalReport {
                eval_id: eval.id.clone(),
                target: eval_target.to_string(),
                passed: missing_expected.is_empty()
                    && present_forbidden.is_empty()
                    && score >= 0.999,
                score,
                missing_expected,
                present_forbidden,
            });
        }
        reports
    }
}

pub fn compile_lsl_roots(
    roots: &[PathBuf],
    compiled_root: &Path,
) -> anyhow::Result<CompiledLslRegistry> {
    let paths = collect_lsl_file_paths(roots)?;
    let workers = ParallelismConfig::detect().constrained_workers(paths.len());
    let compiled_files = run_parallel_ordered(paths, workers, |path| compile_lsl_file(&path));

    let mut libraries = Vec::new();
    let mut hashes = BTreeMap::new();
    for compiled_file in compiled_files {
        let (library, library_hashes) = compiled_file?;
        hashes.insert(library.id.clone(), library_hashes);
        libraries.push(library);
    }

    let compiled = CompiledLslRegistry {
        registry: LslRegistry::from_libraries(libraries),
        hashes,
    };
    write_compiled_registry(&compiled, compiled_root)?;
    Ok(compiled)
}

pub fn load_compiled_registry(compiled_root: &Path) -> anyhow::Result<CompiledLslRegistry> {
    let index = compiled_root.join("index");
    let hash_dir = compiled_root.join("hashes");
    let libraries = read_json(index.join("libraries.json"))?;
    let subskills = read_json(index.join("subskills.json"))?;
    let links = read_json(index.join("links.json"))?;
    let policies = read_json(index.join("policies.json"))?;
    let evals = read_json(index.join("evals.json"))?;
    let hashes = read_json(hash_dir.join("hashes.json"))?;
    let mut registry = LslRegistry {
        libraries,
        subskills,
        links,
        policies,
        evals,
        issues: Vec::new(),
    };
    registry.validate_references();
    Ok(CompiledLslRegistry { registry, hashes })
}

pub fn load_or_compile_lsl_roots(
    roots: &[PathBuf],
    compiled_root: &Path,
) -> anyhow::Result<CompiledLslRegistry> {
    if lsl_registry_status(roots, compiled_root)?.fresh {
        return load_compiled_registry(compiled_root);
    }
    compile_lsl_roots(roots, compiled_root)
}

pub fn lsl_registry_status(
    roots: &[PathBuf],
    compiled_root: &Path,
) -> anyhow::Result<LslRegistryStatus> {
    let compiled_exists = compiled_root.join("index").join("subskills.json").exists()
        && compiled_root.join("hashes").join("hashes.json").exists();
    let source_hashes = collect_lsl_source_hashes(roots)?;
    if !compiled_exists {
        return Ok(LslRegistryStatus {
            compiled_exists,
            fresh: false,
            source_count: source_hashes.len(),
            compiled_source_count: 0,
            ..LslRegistryStatus::default()
        });
    }

    let compiled_hashes: BTreeMap<String, LslHashes> =
        read_json(compiled_root.join("hashes").join("hashes.json"))?;
    let compiled_by_path = compiled_hashes
        .values()
        .filter(|hash| !hash.source_path.is_empty())
        .map(|hash| (hash.source_path.clone(), hash.source_hash.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut stale_sources = Vec::new();
    let mut missing_sources = Vec::new();
    for (path, source_hash) in &source_hashes {
        match compiled_by_path.get(path) {
            Some(compiled_hash) if compiled_hash == source_hash => {}
            Some(_) => stale_sources.push(path.clone()),
            None => missing_sources.push(path.clone()),
        }
    }
    let extra_compiled_sources = compiled_by_path
        .keys()
        .filter(|path| !source_hashes.contains_key(*path))
        .cloned()
        .collect::<Vec<_>>();
    let fresh =
        stale_sources.is_empty() && missing_sources.is_empty() && extra_compiled_sources.is_empty();

    Ok(LslRegistryStatus {
        compiled_exists,
        fresh,
        source_count: source_hashes.len(),
        compiled_source_count: compiled_by_path.len(),
        stale_sources,
        missing_sources,
        extra_compiled_sources,
    })
}

impl CompiledLslRegistry {
    pub fn skill_index(&self) -> Value {
        json!(
            self.registry
                .libraries
                .values()
                .map(|library| {
                    json!({
                        "id": library.id,
                        "name": library.name,
                        "version": library.version,
                        "status": library.status,
                        "risk": library.risk,
                        "subskill_count": library.subskills.len(),
                    })
                })
                .collect::<Vec<_>>()
        )
    }

    pub fn subskill_index(&self) -> Value {
        json!(self.registry.subskills.iter().map(|(id, entry)| {
            json!({
                "id": id,
                "library_id": entry.library_id,
                "title": entry.subskill.title,
                "summary": entry.subskill.summary,
                "tags": entry.subskill.tags,
                "risk": entry.subskill.risk,
                "status": entry.subskill.status,
                "dependencies": entry.subskill.required_concepts,
                "required_tools": entry.subskill.required_tools,
                "token_cost": entry.index.as_ref().map(|index| index.token_cost.clone()).unwrap_or_default(),
                "policy_id": entry.subskill.policy.inherits,
            })
        }).collect::<Vec<_>>())
    }

    pub fn dependency_graph(&self) -> Value {
        json!({
            "links": self.registry.links,
            "required_concepts": self.registry.subskills.iter().map(|(id, entry)| {
                json!({ "from": id, "requires": entry.subskill.required_concepts })
            }).collect::<Vec<_>>()
        })
    }

    pub fn token_map(&self) -> Value {
        json!(self.registry.subskills.iter().map(|(id, entry)| {
            let measured = ["card", "body", "extended"].into_iter().map(|mode| {
                (mode.to_string(), estimate_tokens(&materialize_subskill(&entry.subskill, mode)))
            }).collect::<BTreeMap<_, _>>();
            json!({
                "id": id,
                "declared": entry.subskill.context_budget,
                "index_token_cost": entry.index.as_ref().map(|index| index.token_cost.clone()).unwrap_or_default(),
                "measured": measured,
                "rolling_average": metric_u64(&entry.subskill.metrics, "average_token_cost"),
                "use_count": metric_u64(&entry.subskill.metrics, "use_count"),
                "last_used_at": entry.subskill.metrics.get("last_used_at"),
            })
        }).collect::<Vec<_>>())
    }

    pub fn embedding_index(&self) -> Value {
        json!(
            self.registry
                .subskills
                .iter()
                .map(|(id, entry)| {
                    let text = [
                        entry.subskill.title.clone().unwrap_or_default(),
                        entry.subskill.summary.clone().unwrap_or_default(),
                        entry.subskill.tags.join(" "),
                        entry.subskill.load.get("card").cloned().unwrap_or_default(),
                    ]
                    .join(" ");
                    json!({
                        "id": id,
                        "provider": "lexical-token-fingerprint",
                        "embedding_available": false,
                        "tokens": terms(&text).into_iter().collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>()
        )
    }

    pub fn policy_map(&self) -> Value {
        json!(
            self.registry
                .subskills
                .iter()
                .map(|(id, entry)| {
                    let effective = self
                        .registry
                        .libraries
                        .get(&entry.library_id)
                        .and_then(|library| {
                            effective_subskill_policy(library, &entry.subskill).ok()
                        })
                        .unwrap_or_else(|| entry.subskill.policy.clone());
                    json!({
                        "id": id,
                        "library_id": entry.library_id,
                        "risk": entry.subskill.risk,
                        "subskill_policy": entry.subskill.policy,
                        "effective_policy": effective,
                        "approval_required": !effective.requires_approval.is_empty(),
                    })
                })
                .collect::<Vec<_>>()
        )
    }
}

pub fn write_compiled_registry(
    compiled: &CompiledLslRegistry,
    compiled_root: &Path,
) -> anyhow::Result<()> {
    let usrl_ast = compiled_root.join("usrl_ast");
    let index = compiled_root.join("index");
    let hash_dir = compiled_root.join("hashes");
    fs::create_dir_all(&usrl_ast)?;
    fs::create_dir_all(&index)?;
    fs::create_dir_all(&hash_dir)?;

    for library in compiled.registry.libraries.values() {
        write_json(
            &usrl_ast.join(format!("{}.ast.json", library.id)),
            &canonical_lsl_value(library)?,
        )?;
    }
    write_json(&index.join("libraries.json"), &compiled.registry.libraries)?;
    write_json(&index.join("subskills.json"), &compiled.registry.subskills)?;
    write_json(&index.join("links.json"), &compiled.registry.links)?;
    write_json(&index.join("policies.json"), &compiled.registry.policies)?;
    write_json(&index.join("evals.json"), &compiled.registry.evals)?;
    write_json(&index.join("skill_index.json"), &compiled.skill_index())?;
    write_json(
        &index.join("subskill_index.json"),
        &compiled.subskill_index(),
    )?;
    write_json(
        &index.join("dependency_graph.json"),
        &compiled.dependency_graph(),
    )?;
    write_json(&index.join("token_map.json"), &compiled.token_map())?;
    write_json(&index.join("policy_map.json"), &compiled.policy_map())?;
    write_json(&index.join("eval_index.json"), &compiled.registry.evals)?;
    write_json(
        &index.join("subskill_embeddings.json"),
        &compiled.embedding_index(),
    )?;
    write_json(&hash_dir.join("hashes.json"), &compiled.hashes)?;
    Ok(())
}

pub fn canonical_lsl_text(library: &LinkedSkillLibrary) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(&canonical_lsl_value(
        library,
    )?)?)
}

pub fn semantic_lsl_text(library: &LinkedSkillLibrary) -> anyhow::Result<String> {
    let mut value = canonical_lsl_value(library)?;
    if let Value::Object(object) = &mut value {
        object.remove("metadata");
        object.remove("source_hash");
        object.remove("canonical_hash");
        object.remove("semantic_hash");
        if let Some(Value::Array(subskills)) = object.get_mut("subskills") {
            for subskill in subskills {
                if let Value::Object(subskill) = subskill {
                    subskill.remove("metrics");
                }
            }
        }
    }
    Ok(serde_json::to_string_pretty(&value)?)
}

pub fn subskill_metadata(
    library: &LinkedSkillLibrary,
    subskill: &LslSubskill,
    source_path: &str,
    source: &str,
) -> BTreeMap<String, Value> {
    let mut metadata = BTreeMap::new();
    let effective_policy = effective_subskill_policy(library, subskill).unwrap_or_default();
    metadata.insert("path".to_string(), Value::String(source_path.to_string()));
    metadata.insert("format".to_string(), Value::String("lsl".to_string()));
    metadata.insert(
        "body".to_string(),
        Value::String(render_subskill_context(subskill)),
    );
    metadata.insert("library_id".to_string(), Value::String(library.id.clone()));
    metadata.insert(
        "library_name".to_string(),
        Value::String(library.name.clone()),
    );
    metadata.insert(
        "subskill_id".to_string(),
        Value::String(subskill.id.clone()),
    );
    metadata.insert("source".to_string(), Value::String(source.to_string()));
    metadata.insert("tags".to_string(), json_string_array(subskill.tags.clone()));
    metadata.insert(
        "activation_positive".to_string(),
        json_string_array(subskill.activation_positive.clone()),
    );
    metadata.insert(
        "activation_negative".to_string(),
        json_string_array(subskill.activation_negative.clone()),
    );
    metadata.insert(
        "required_concepts".to_string(),
        json_string_array(subskill.required_concepts.clone()),
    );
    metadata.insert(
        "required_tools".to_string(),
        json_string_array(subskill.required_tools.clone()),
    );
    metadata.insert(
        "forbidden".to_string(),
        json_string_array(subskill.forbidden.clone()),
    );
    metadata.insert(
        "verification".to_string(),
        json_string_array(subskill.verification.clone()),
    );
    metadata.insert(
        "failure_modes".to_string(),
        json_string_array(subskill.failure_modes.clone()),
    );
    metadata.insert("policy".to_string(), json!(subskill.policy));
    metadata.insert("effective_policy".to_string(), json!(effective_policy));
    metadata.insert("load".to_string(), json!(subskill.load));
    metadata.insert("context_budget".to_string(), json!(subskill.context_budget));
    metadata.insert(
        "links".to_string(),
        json!(
            library
                .links
                .iter()
                .filter(|link| link.from == subskill.id || link.to == subskill.id)
                .collect::<Vec<_>>()
        ),
    );
    metadata.insert(
        "eval_refs".to_string(),
        json_string_array(subskill.eval_refs.clone()),
    );
    metadata
}

pub fn materialize_subskill(subskill: &LslSubskill, mode: &str) -> String {
    let mut sections = Vec::new();
    sections.push(format!(
        "Sub-skill: {}\n{}",
        subskill.id,
        subskill
            .summary
            .as_deref()
            .or(subskill.title.as_deref())
            .unwrap_or("No summary provided.")
    ));
    if !subskill.tags.is_empty() {
        sections.push(format!("Tags: {}", subskill.tags.join(", ")));
    }
    if mode != "card" && (!subskill.inputs.is_empty() || !subskill.outputs.is_empty()) {
        let inputs = subskill
            .inputs
            .iter()
            .map(|field| {
                format!(
                    "{}: {} {}",
                    field.name,
                    field.field_type,
                    if field.required {
                        "required"
                    } else {
                        "optional"
                    }
                )
            })
            .collect::<Vec<_>>();
        let outputs = subskill
            .outputs
            .iter()
            .map(|field| format!("{}: {}", field.name, field.field_type))
            .collect::<Vec<_>>();
        sections.push(format!(
            "Callable signature:\nInputs: {}\nOutputs: {}",
            if inputs.is_empty() {
                "-".to_string()
            } else {
                inputs.join(", ")
            },
            if outputs.is_empty() {
                "-".to_string()
            } else {
                outputs.join(", ")
            }
        ));
    }
    for load_mode in modes_for(mode) {
        if let Some(text) = subskill.load.get(load_mode) {
            sections.push(format!("{}:\n{}", title_case(load_mode), text.trim()));
        }
    }
    if mode != "card" {
        if !subskill.forbidden.is_empty() {
            sections.push(format!("Forbidden:\n{}", bullet_list(&subskill.forbidden)));
        }
        if !subskill.verification.is_empty() {
            sections.push(format!(
                "Verification:\n{}",
                bullet_list(&subskill.verification)
            ));
        }
        if !subskill.failure_modes.is_empty() {
            sections.push(format!(
                "Failure modes:\n{}",
                bullet_list(&subskill.failure_modes)
            ));
        }
    }
    sections.join("\n\n")
}

pub fn effective_subskill_policy(
    library: &LinkedSkillLibrary,
    subskill: &LslSubskill,
) -> anyhow::Result<LslPolicy> {
    resolve_policy(library, &subskill.policy, &mut BTreeSet::new())
}

pub fn forge_candidate_subskill(root: &Path, draft: LslSkillDraft) -> anyhow::Result<PathBuf> {
    validate_subskill_id(&draft.id)?;
    let library_id = draft
        .id
        .split('.')
        .next()
        .filter(|part| !part.is_empty())
        .context("subskill id must include library namespace")?
        .to_string();
    fs::create_dir_all(root)?;
    let path = root.join(format!("{library_id}.lsl"));
    let mut library = if path.exists() {
        parse_lsl(&fs::read_to_string(&path)?)
            .with_context(|| format!("failed to parse {}", path.display()))?
    } else {
        empty_library(&library_id)
    };
    if library
        .subskills
        .iter()
        .any(|subskill| subskill.id == draft.id)
    {
        bail!("subskill '{}' already exists", draft.id);
    }

    let card = draft.summary.clone();
    let eval_id = format!("{}.eval.candidate_001", draft.id);
    let mut token_cost = BTreeMap::new();
    token_cost.insert("card".to_string(), estimate_tokens(&card) as u64);
    token_cost.insert("body".to_string(), estimate_tokens(&draft.body) as u64);
    token_cost.insert(
        "extended".to_string(),
        estimate_tokens(&format!("{}\n{}", card, draft.body)) as u64,
    );
    library.index.insert(
        draft.id.clone(),
        LslIndexItem {
            id: draft.id.clone(),
            title: Some(draft.title.clone()),
            summary: Some(draft.summary.clone()),
            tags: draft.tags.clone(),
            risk: Some("low".to_string()),
            token_cost,
        },
    );

    let mut load = BTreeMap::new();
    load.insert("card".to_string(), card);
    load.insert("body".to_string(), draft.body.clone());
    let mut context_budget = BTreeMap::new();
    context_budget.insert("card_tokens".to_string(), 100);
    context_budget.insert("body_tokens".to_string(), 800);
    context_budget.insert("extended_tokens".to_string(), 1600);
    let mut metrics = BTreeMap::new();
    metrics.insert("use_count".to_string(), Value::Number(0.into()));
    metrics.insert("success_count".to_string(), Value::Number(0.into()));
    metrics.insert("failure_count".to_string(), Value::Number(0.into()));
    metrics.insert("eval_score".to_string(), json!(0.0));
    metrics.insert("average_token_cost".to_string(), Value::Number(0.into()));
    metrics.insert(
        "provenance".to_string(),
        Value::String(draft.provenance.clone()),
    );

    library.subskills.push(LslSubskill {
        id: draft.id.clone(),
        title: Some(draft.title.clone()),
        version: Some("0.1.0".to_string()),
        status: Some("candidate".to_string()),
        skill_type: Some("procedure".to_string()),
        risk: Some("low".to_string()),
        summary: Some(draft.summary.clone()),
        tags: draft.tags.clone(),
        activation_positive: draft.tags.clone(),
        activation_negative: Vec::new(),
        inputs: vec![LslSignatureField {
            name: "task".to_string(),
            field_type: "string".to_string(),
            required: true,
        }],
        outputs: vec![LslSignatureField {
            name: "guidance".to_string(),
            field_type: "checklist".to_string(),
            required: false,
        }],
        required_concepts: Vec::new(),
        required_tools: Vec::new(),
        policy: LslPolicy {
            inherits: Some(format!("{library_id}.policy.default")),
            ..LslPolicy::default()
        },
        context_budget,
        load,
        forbidden: Vec::new(),
        verification: vec![
            "The guidance matches the task context.".to_string(),
            "The answer identifies relevant failure modes.".to_string(),
        ],
        failure_modes: vec![
            "Overgeneralizing from one trace.".to_string(),
            "Missing environment-specific constraints.".to_string(),
        ],
        metrics,
        eval_refs: vec![eval_id.clone()],
    });
    library.evals.push(LslEval {
        id: eval_id,
        target: Some(draft.id.clone()),
        task: Some(format!("Evaluate candidate sub-skill {}", draft.id)),
        expected: vec![draft.summary.clone()],
        forbidden: Vec::new(),
        scoring: [
            ("correctness".to_string(), 0.7),
            ("safety".to_string(), 0.3),
        ]
        .into_iter()
        .collect(),
    });

    let source = render_lsl_library(&library)?;
    parse_lsl(&source)?;
    fs::write(&path, source).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

pub fn set_subskill_status(root: &Path, id: &str, status: &str) -> anyhow::Result<PathBuf> {
    validate_optional_enum(
        "subskill status",
        &Some(status.to_string()),
        &[
            "candidate",
            "sandboxed",
            "evaluated",
            "active",
            "stale",
            "archived",
            "pinned",
        ],
    )?;
    let library_id = id
        .split('.')
        .next()
        .filter(|part| !part.is_empty())
        .context("subskill id must include library namespace")?;
    let path = root.join(format!("{library_id}.lsl"));
    let mut library = parse_lsl(&fs::read_to_string(&path)?)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let Some(subskill) = library
        .subskills
        .iter_mut()
        .find(|subskill| subskill.id == id)
    else {
        bail!("subskill '{id}' not found");
    };
    subskill.status = Some(status.to_string());
    let source = render_lsl_library(&library)?;
    parse_lsl(&source)?;
    fs::write(&path, source).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

pub fn transition_subskill_status(
    root: &Path,
    id: &str,
    status: &str,
    eval_reports: &[LslEvalReport],
    override_direct: bool,
) -> anyhow::Result<PathBuf> {
    validate_optional_enum(
        "subskill status",
        &Some(status.to_string()),
        &[
            "candidate",
            "sandboxed",
            "evaluated",
            "active",
            "stale",
            "archived",
            "pinned",
        ],
    )?;
    validate_subskill_id(id)?;
    let library_id = id
        .split('.')
        .next()
        .filter(|part| !part.is_empty())
        .context("subskill id must include library namespace")?;
    let path = root.join(format!("{library_id}.lsl"));
    let mut library = parse_lsl(&fs::read_to_string(&path)?)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let Some(subskill) = library
        .subskills
        .iter_mut()
        .find(|subskill| subskill.id == id)
    else {
        bail!("subskill '{id}' not found");
    };
    let current = subskill.status.as_deref().unwrap_or("active");
    validate_lifecycle_transition(current, status, eval_reports, override_direct)?;
    subskill.status = Some(status.to_string());
    if status == "evaluated" || status == "active" {
        let score = eval_reports
            .iter()
            .filter(|report| report.target == id)
            .map(|report| report.score)
            .fold(1.0_f64, f64::min);
        subskill
            .metrics
            .insert("eval_score".to_string(), json!(score));
        subskill.metrics.insert(
            "evaluated_at".to_string(),
            Value::String(chrono::Utc::now().to_rfc3339()),
        );
    }
    if status == "active" {
        subskill.metrics.insert(
            "promoted_at".to_string(),
            Value::String(chrono::Utc::now().to_rfc3339()),
        );
    }
    let source = render_lsl_library(&library)?;
    parse_lsl(&source)?;
    fs::write(&path, source).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn validate_lifecycle_transition(
    current: &str,
    target: &str,
    eval_reports: &[LslEvalReport],
    override_direct: bool,
) -> anyhow::Result<()> {
    if current == target {
        return Ok(());
    }
    let evals_pass = !eval_reports.is_empty() && eval_reports.iter().all(|report| report.passed);
    let allowed = matches!(
        (current, target),
        ("candidate", "sandboxed")
            | ("sandboxed", "evaluated")
            | ("evaluated", "active")
            | ("active", "stale")
            | ("stale", "archived")
            | ("active", "pinned")
            | ("pinned", "active")
            | (_, "archived")
    ) || (override_direct && target == "active");
    if !allowed {
        bail!(
            "invalid lifecycle transition {current} -> {target}; expected candidate -> sandboxed -> evaluated -> active unless override is explicit"
        );
    }
    if matches!(target, "evaluated" | "active") && !evals_pass {
        bail!("transition to {target} requires passing eval reports");
    }
    Ok(())
}

pub fn patch_subskill(root: &Path, patch: LslPatchRequest) -> anyhow::Result<PathBuf> {
    validate_subskill_id(&patch.target)?;
    let library_id = patch.target.split('.').next().unwrap_or_default();
    let path = root.join(format!("{library_id}.lsl"));
    let mut library = parse_lsl(&fs::read_to_string(&path)?)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let Some(subskill) = library
        .subskills
        .iter_mut()
        .find(|subskill| subskill.id == patch.target)
    else {
        bail!("subskill '{}' not found", patch.target);
    };
    match (patch.operation.as_str(), patch.path.as_str()) {
        ("replace_field", "summary") => subskill.summary = Some(patch.value),
        ("replace_field", "load.card") => {
            subskill.load.insert("card".to_string(), patch.value);
        }
        ("replace_field", "load.body") => {
            subskill.load.insert("body".to_string(), patch.value);
        }
        ("replace_field", "load.extended") => {
            subskill.load.insert("extended".to_string(), patch.value);
        }
        ("append_list_items", "verification") => {
            append_unique(&mut subskill.verification, patch.value)
        }
        ("append_list_items", "failure_modes") => {
            append_unique(&mut subskill.failure_modes, patch.value)
        }
        ("append_list_items", "activation.positive") => {
            append_unique(&mut subskill.activation_positive, patch.value)
        }
        ("append_list_items", "activation.negative") => {
            append_unique(&mut subskill.activation_negative, patch.value)
        }
        ("append_list_items", "tags") => append_unique(&mut subskill.tags, patch.value),
        _ => bail!(
            "unsupported patch operation '{}' for path '{}'",
            patch.operation,
            patch.path
        ),
    }
    let source = render_lsl_library(&library)?;
    parse_lsl(&source)?;
    fs::write(&path, source).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

pub fn update_skill_metrics_for_load(
    root: &Path,
    selected: &[LoadedSubskill],
    success: Option<bool>,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut by_library: BTreeMap<String, Vec<&LoadedSubskill>> = BTreeMap::new();
    for loaded in selected {
        let Some(library_id) = loaded.id.split('.').next().filter(|part| !part.is_empty()) else {
            continue;
        };
        by_library
            .entry(library_id.to_string())
            .or_default()
            .push(loaded);
    }
    let mut written = Vec::new();
    for (library_id, loads) in by_library {
        let path = root.join(format!("{library_id}.lsl"));
        if !path.exists() {
            continue;
        }
        let mut library = parse_lsl(&fs::read_to_string(&path)?)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        for loaded in loads {
            if let Some(subskill) = library
                .subskills
                .iter_mut()
                .find(|subskill| subskill.id == loaded.id)
            {
                increment_metric(&mut subskill.metrics, "use_count", 1);
                match success {
                    Some(true) => increment_metric(&mut subskill.metrics, "success_count", 1),
                    Some(false) => increment_metric(&mut subskill.metrics, "failure_count", 1),
                    None => {}
                }
                update_average_metric(
                    &mut subskill.metrics,
                    "average_token_cost",
                    loaded.token_estimate as u64,
                );
                subskill.metrics.insert(
                    "last_used_at".to_string(),
                    Value::String(chrono::Utc::now().to_rfc3339()),
                );
                subskill.metrics.insert(
                    "last_load_mode".to_string(),
                    Value::String(loaded.mode.clone()),
                );
            }
        }
        let source = render_lsl_library(&library)?;
        parse_lsl(&source)?;
        fs::write(&path, source).with_context(|| format!("failed to write {}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

fn increment_metric(metrics: &mut BTreeMap<String, Value>, key: &str, amount: u64) {
    let next = metrics.get(key).and_then(Value::as_u64).unwrap_or_default() + amount;
    metrics.insert(key.to_string(), Value::Number(next.into()));
}

fn update_average_metric(metrics: &mut BTreeMap<String, Value>, key: &str, sample: u64) {
    let count = metrics
        .get("use_count")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1);
    let previous = metrics.get(key).and_then(Value::as_u64).unwrap_or(sample);
    let next = ((previous.saturating_mul(count.saturating_sub(1))) + sample) / count;
    metrics.insert(key.to_string(), Value::Number(next.into()));
}

pub fn append_skill_trace(path: &Path, trace: LslSkillTrace) -> anyhow::Result<()> {
    let mut traces = read_skill_traces(path).unwrap_or_default();
    traces.push(trace);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&traces)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

pub fn read_skill_traces(path: &Path) -> anyhow::Result<Vec<LslSkillTrace>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json(path)
}

pub fn curate_registry(
    registry: &LslRegistry,
    traces: &[LslSkillTrace],
    eval_reports: &[LslEvalReport],
) -> LslCuratorReport {
    let mut report = LslCuratorReport {
        total_subskills: registry.subskills.len(),
        ..LslCuratorReport::default()
    };
    for (id, entry) in &registry.subskills {
        match entry.subskill.status.as_deref().unwrap_or("active") {
            "active" => report.active_subskills += 1,
            "candidate" => report.candidate_subskills.push(id.clone()),
            "stale" => report.stale_subskills.push(id.clone()),
            "archived" => report.archived_subskills.push(id.clone()),
            _ => {}
        }
    }

    let mut by_summary: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, entry) in &registry.subskills {
        if let Some(summary) = &entry.subskill.summary {
            by_summary
                .entry(summary.to_ascii_lowercase())
                .or_default()
                .push(id.clone());
        }
    }
    report.duplicate_summary_groups = by_summary
        .into_values()
        .filter(|items| items.len() > 1)
        .collect();

    report.failing_evals = eval_reports
        .iter()
        .filter(|report| !report.passed)
        .map(|report| format!("{} ({:.2})", report.eval_id, report.score))
        .collect();

    let mut use_counts = registry
        .subskills
        .keys()
        .map(|id| {
            let count = traces
                .iter()
                .filter(|trace| trace.selected.iter().any(|selected| selected == id))
                .count();
            (count, id.clone())
        })
        .collect::<Vec<_>>();
    use_counts.sort();
    report.least_used_subskills = use_counts
        .into_iter()
        .take(5)
        .filter(|(count, _)| *count == 0)
        .map(|(_, id)| id)
        .collect();
    report.missing_skill_candidates = detect_missing_skill_candidates(traces);
    for id in &report.candidate_subskills {
        report.recommendations.push(LslCuratorRecommendation {
            kind: "add_eval_or_sandbox".to_string(),
            target: id.clone(),
            reason: "candidate sub-skill is not active until sandboxed/evaluated/promoted"
                .to_string(),
            suggested_action: format!(
                "/skills sandbox {id}; /skills eval {id}; /skills promote {id}"
            ),
        });
    }
    for id in &report.stale_subskills {
        report.recommendations.push(LslCuratorRecommendation {
            kind: "archive_or_patch_stale".to_string(),
            target: id.clone(),
            reason: "sub-skill is marked stale".to_string(),
            suggested_action: format!("/skills archive {id} or /skills patch {id} | replace_field | load.body | <updated body>"),
        });
    }
    for group in &report.duplicate_summary_groups {
        report.recommendations.push(LslCuratorRecommendation {
            kind: "merge_duplicate_subskills".to_string(),
            target: group.join(","),
            reason: "sub-skills share the same summary".to_string(),
            suggested_action: "review for merge, replacement link, or archive one duplicate"
                .to_string(),
        });
    }
    for failing in &report.failing_evals {
        report.recommendations.push(LslCuratorRecommendation {
            kind: "repair_failing_eval".to_string(),
            target: failing.clone(),
            reason: "eval hook is failing".to_string(),
            suggested_action: "patch the sub-skill or eval, then rerun /skills eval".to_string(),
        });
    }
    for missing in &report.missing_skill_candidates {
        report.recommendations.push(LslCuratorRecommendation {
            kind: "forge_missing_skill".to_string(),
            target: missing.clone(),
            reason: "repeated no-match skill traces suggest missing coverage".to_string(),
            suggested_action: "/skills forge <library.subskill> | <title> | <summary> | <body>"
                .to_string(),
        });
    }
    report
}

pub fn detect_missing_skill_candidates(traces: &[LslSkillTrace]) -> Vec<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for trace in traces {
        if !trace.selected.is_empty() || trace.query.trim().is_empty() {
            continue;
        }
        let key = terms(&trace.query)
            .into_iter()
            .take(4)
            .collect::<Vec<_>>()
            .join("_");
        if !key.is_empty() {
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .map(|(key, count)| format!("{key} ({count} no-match traces)"))
        .collect()
}

fn validate_lsl(library: &LinkedSkillLibrary) -> anyhow::Result<()> {
    if library.subskills.is_empty() {
        bail!("library '{}' must define at least one subskill", library.id);
    }
    validate_optional_enum(
        "library status",
        &library.status,
        &["active", "stale", "archived", "pinned"],
    )?;
    validate_optional_enum(
        "library risk",
        &library.risk,
        &["low", "medium", "high", "critical"],
    )?;
    validate_optional_enum(
        "load_policy.default_context_mode",
        &library.load_policy.default_context_mode,
        &["index_only", "card", "body", "extended"],
    )?;
    validate_optional_enum(
        "load_policy.allow_extended_load",
        &library.load_policy.allow_extended_load,
        &["never", "conditional", "always"],
    )?;
    let mut ids = BTreeSet::new();
    for subskill in &library.subskills {
        if !ids.insert(subskill.id.clone()) {
            bail!("duplicate subskill id '{}'", subskill.id);
        }
        validate_optional_enum(
            &format!("subskill '{}' status", subskill.id),
            &subskill.status,
            &[
                "candidate",
                "sandboxed",
                "evaluated",
                "active",
                "stale",
                "archived",
                "pinned",
            ],
        )?;
        validate_optional_enum(
            &format!("subskill '{}' type", subskill.id),
            &subskill.skill_type,
            &[
                "concept",
                "concept_procedure",
                "procedure",
                "diagnostic",
                "workflow",
                "review",
                "implementation",
                "policy",
            ],
        )?;
        validate_optional_enum(
            &format!("subskill '{}' risk", subskill.id),
            &subskill.risk,
            &["low", "medium", "high", "critical"],
        )?;
        if subskill.title.as_deref().unwrap_or_default().is_empty() {
            bail!("subskill '{}' must include title", subskill.id);
        }
        if subskill.summary.as_deref().unwrap_or_default().is_empty() {
            bail!("subskill '{}' must include summary", subskill.id);
        }
        if subskill.load.is_empty() {
            bail!("subskill '{}' must include load blocks", subskill.id);
        }
        if let Some(inherits) = &subskill.policy.inherits
            && !library.policies.contains_key(inherits)
        {
            bail!(
                "subskill '{}' inherits unknown policy '{}'",
                subskill.id,
                inherits
            );
        }
        let effective = effective_subskill_policy(library, subskill)?;
        for allowed in &subskill.policy.allowed {
            if effective
                .forbidden
                .iter()
                .any(|forbidden| forbidden == allowed)
            {
                bail!(
                    "subskill '{}' policy allows forbidden capability '{}'",
                    subskill.id,
                    allowed
                );
            }
        }
        for eval_ref in &subskill.eval_refs {
            if !library.evals.iter().any(|eval| &eval.id == eval_ref) {
                bail!(
                    "subskill '{}' references unknown eval '{}'",
                    subskill.id,
                    eval_ref
                );
            }
        }
    }
    for (id, policy) in &library.policies {
        if let Some(inherits) = &policy.inherits
            && !library.policies.contains_key(inherits)
        {
            bail!("policy '{}' inherits unknown policy '{}'", id, inherits);
        }
        let effective = resolve_policy(library, policy, &mut BTreeSet::new())?;
        for allowed in &policy.allowed {
            if effective
                .forbidden
                .iter()
                .any(|forbidden| forbidden == allowed)
            {
                bail!(
                    "policy '{}' allows inherited forbidden capability '{}'",
                    id,
                    allowed
                );
            }
        }
    }
    for link in &library.links {
        if link.from.is_empty() || link.to.is_empty() {
            bail!("link '{}' must include from and to", link.id);
        }
        validate_optional_enum(
            &format!("link '{}' relation", link.id),
            &link.relation,
            &[
                "requires",
                "related",
                "conflicts",
                "extends",
                "replaces",
                "fallback",
                "specializes",
                "generalizes",
            ],
        )?;
        validate_optional_enum(
            &format!("link '{}' load_hint", link.id),
            &link.load_hint,
            &["none", "card", "body", "extended"],
        )?;
        if let Some(strength) = link.strength
            && !(0.0..=1.0).contains(&strength)
        {
            bail!("link '{}' strength must be between 0.0 and 1.0", link.id);
        }
        if !ids.contains(&link.from) {
            bail!(
                "link '{}' references unknown source '{}'",
                link.id,
                link.from
            );
        }
        if !ids.contains(&link.to) {
            bail!("link '{}' references unknown target '{}'", link.id, link.to);
        }
    }
    for eval in &library.evals {
        if eval.expected.is_empty() {
            bail!("eval '{}' must include expected checks", eval.id);
        }
        let total_weight: f64 = eval.scoring.values().sum();
        if total_weight > 0.0 && (total_weight - 1.0).abs() > 0.001 {
            bail!("eval '{}' scoring weights must sum to 1.0", eval.id);
        }
        if let Some(target) = &eval.target
            && !ids.contains(target)
        {
            bail!("eval '{}' references unknown target '{}'", eval.id, target);
        }
    }
    Ok(())
}

fn validate_optional_enum(
    name: &str,
    value: &Option<String>,
    allowed: &[&str],
) -> anyhow::Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if !allowed.iter().any(|allowed| allowed == value) {
        bail!(
            "{name} has invalid value '{}'; expected one of {}",
            value,
            allowed.join(", ")
        );
    }
    Ok(())
}

fn validate_subskill_id(id: &str) -> anyhow::Result<()> {
    if id.split('.').count() < 2 {
        bail!("subskill id must use dotted namespace");
    }
    for part in id.split('.') {
        if part.is_empty()
            || !part
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        {
            bail!("invalid subskill id '{id}'; use lowercase dotted identifiers");
        }
    }
    Ok(())
}

fn render_subskill_context(subskill: &LslSubskill) -> String {
    materialize_subskill(subskill, "body")
}

fn resolve_policy(
    library: &LinkedSkillLibrary,
    policy: &LslPolicy,
    seen: &mut BTreeSet<String>,
) -> anyhow::Result<LslPolicy> {
    let mut effective = LslPolicy {
        id: policy.id.clone(),
        inherits: policy.inherits.clone(),
        allowed: Vec::new(),
        requires_approval: Vec::new(),
        forbidden: Vec::new(),
    };
    if let Some(inherits) = &policy.inherits {
        if !seen.insert(inherits.clone()) {
            bail!("policy inheritance cycle at '{}'", inherits);
        }
        let parent = library
            .policies
            .get(inherits)
            .with_context(|| format!("unknown inherited policy '{inherits}'"))?;
        let parent = resolve_policy(library, parent, seen)?;
        extend_unique(&mut effective.allowed, parent.allowed);
        extend_unique(&mut effective.requires_approval, parent.requires_approval);
        extend_unique(&mut effective.forbidden, parent.forbidden);
    }
    extend_unique(&mut effective.allowed, policy.allowed.clone());
    extend_unique(
        &mut effective.requires_approval,
        policy.requires_approval.clone(),
    );
    extend_unique(&mut effective.forbidden, policy.forbidden.clone());
    Ok(effective)
}

fn extend_unique(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

fn append_unique(target: &mut Vec<String>, raw: String) {
    for value in raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

fn term_overlap(query_terms: &BTreeSet<String>, values: &[String]) -> usize {
    let value_terms = terms(&values.join(" "));
    query_terms.intersection(&value_terms).count()
}

fn metric_u64(metrics: &BTreeMap<String, Value>, key: &str) -> u64 {
    metrics.get(key).and_then(Value::as_u64).unwrap_or_default()
}

fn budgeted_materialization(
    subskill: &LslSubskill,
    preferred_mode: &str,
    remaining_tokens: usize,
    allow_oversized_first: bool,
) -> (String, String, usize) {
    let modes = match preferred_mode {
        "extended" => ["extended", "body", "card"].as_slice(),
        "body" => ["body", "card"].as_slice(),
        _ => ["card"].as_slice(),
    };
    let mut fallback = None;
    for mode in modes {
        let text = materialize_subskill(subskill, mode);
        let estimate = token_cost_for(subskill, mode).unwrap_or_else(|| estimate_tokens(&text));
        fallback.get_or_insert_with(|| ((*mode).to_string(), text.clone(), estimate));
        if estimate <= remaining_tokens || allow_oversized_first {
            return ((*mode).to_string(), text, estimate);
        }
    }
    fallback.unwrap_or_else(|| {
        let text = materialize_subskill(subskill, "card");
        let estimate = token_cost_for(subskill, "card").unwrap_or_else(|| estimate_tokens(&text));
        ("card".to_string(), text, estimate)
    })
}

fn token_cost_for(subskill: &LslSubskill, mode: &str) -> Option<usize> {
    let keys = match mode {
        "extended" => ["extended_tokens", "extended", "body_tokens", "body"].as_slice(),
        "body" => ["body_tokens", "body"].as_slice(),
        _ => ["card_tokens", "card"].as_slice(),
    };
    keys.iter()
        .find_map(|key| subskill.context_budget.get(*key).copied())
        .map(|value| value as usize)
}

fn semantic_route_score(query: &str, entry: &LslRegistryEntry) -> f64 {
    // Deterministic local semantic-ish fallback: token Jaccard over compact routing text.
    // Embedding providers can replace this signal without changing route output shape.
    let mut searchable = Vec::new();
    searchable.push(entry.subskill.id.clone());
    searchable.extend(entry.subskill.title.clone());
    searchable.extend(entry.subskill.summary.clone());
    searchable.extend(entry.subskill.tags.clone());
    searchable.extend(entry.subskill.activation_positive.clone());
    searchable.extend(entry.subskill.load.get("card").cloned());
    if let Some(index) = &entry.index {
        searchable.extend(index.title.clone());
        searchable.extend(index.summary.clone());
        searchable.extend(index.tags.clone());
    }
    let query_terms = terms(query);
    let text_terms = terms(&searchable.join(" "));
    if query_terms.is_empty() || text_terms.is_empty() {
        return 0.0;
    }
    let intersection = query_terms.intersection(&text_terms).count() as f64;
    let union = query_terms.union(&text_terms).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn tool_compatibility_score(required_tools: &[String]) -> usize {
    if required_tools.is_empty() { 1 } else { 0 }
}

fn risk_penalty(risk: Option<&str>) -> usize {
    match risk.unwrap_or("medium") {
        "critical" => 3,
        "high" => 2,
        _ => 0,
    }
}

fn route_score(query_terms: &BTreeSet<String>, entry: &LslRegistryEntry) -> usize {
    let mut searchable = Vec::new();
    searchable.push(entry.subskill.id.clone());
    searchable.extend(entry.subskill.title.clone());
    searchable.extend(entry.subskill.summary.clone());
    searchable.extend(entry.subskill.tags.clone());
    searchable.extend(entry.subskill.activation_positive.clone());
    if let Some(index) = &entry.index {
        searchable.extend(index.title.clone());
        searchable.extend(index.summary.clone());
        searchable.extend(index.tags.clone());
    }
    let text_terms = terms(&searchable.join(" "));
    query_terms.intersection(&text_terms).count()
}

fn terms(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn modes_for(mode: &str) -> Vec<&'static str> {
    match mode {
        "extended" => vec!["card", "body", "extended"],
        "body" => vec!["card", "body"],
        _ => vec!["card"],
    }
}

fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

fn bullet_list(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

fn eval_score(eval: &LslEval, missing_expected: &[String], present_forbidden: &[String]) -> f64 {
    let correctness = eval.scoring.get("correctness").copied().unwrap_or(0.5);
    let safety = eval.scoring.get("safety").copied().unwrap_or(0.5);
    let expected_total = eval.expected.len().max(1) as f64;
    let expected_present = eval.expected.len().saturating_sub(missing_expected.len()) as f64;
    let expected_score = expected_present / expected_total;
    let forbidden_total = eval.forbidden.len().max(1) as f64;
    let forbidden_penalty = present_forbidden.len() as f64 / forbidden_total;
    ((expected_score * correctness) + ((1.0 - forbidden_penalty) * safety)).clamp(0.0, 1.0)
}

fn empty_library(id: &str) -> LinkedSkillLibrary {
    let mut metadata = BTreeMap::new();
    metadata.insert("id".to_string(), Value::String(id.to_string()));
    metadata.insert("name".to_string(), Value::String(title_case(id)));
    metadata.insert("version".to_string(), Value::String("0.1.0".to_string()));
    metadata.insert("status".to_string(), Value::String("active".to_string()));
    metadata.insert("risk".to_string(), Value::String("low".to_string()));
    let policy_id = format!("{id}.policy.default");
    let mut policies = BTreeMap::new();
    policies.insert(
        policy_id.clone(),
        LslPolicy {
            id: policy_id,
            allowed: vec![
                "education".to_string(),
                "defensive_design".to_string(),
                "implementation_review".to_string(),
            ],
            requires_approval: Vec::new(),
            forbidden: vec!["unsafe_or_unauthorized_action".to_string()],
            ..LslPolicy::default()
        },
    );
    LinkedSkillLibrary {
        id: id.to_string(),
        name: title_case(id),
        version: Some("0.1.0".to_string()),
        status: Some("active".to_string()),
        risk: Some("low".to_string()),
        metadata,
        load_policy: LslLoadPolicy {
            default_context_mode: Some("index_only".to_string()),
            max_primary_subskills: Some(3),
            max_total_subskills: Some(8),
            require_dependency_closure: Some(true),
            allow_extended_load: Some("conditional".to_string()),
        },
        policies,
        ..LinkedSkillLibrary::default()
    }
}

fn render_lsl_library(library: &LinkedSkillLibrary) -> anyhow::Result<String> {
    let mut out = String::new();
    out.push_str(&format!("library {} {{\n", library.id));
    out.push_str("    meta {\n");
    out.push_str(&format!(
        "        id: \"{}\";\n",
        escape_string(&library.id)
    ));
    out.push_str(&format!(
        "        name: \"{}\";\n",
        escape_string(&library.name)
    ));
    if let Some(version) = &library.version {
        out.push_str(&format!(
            "        version: \"{}\";\n",
            escape_string(version)
        ));
    }
    if let Some(status) = &library.status {
        out.push_str(&format!("        status: {};\n", status));
    }
    if let Some(risk) = &library.risk {
        out.push_str(&format!("        risk: {};\n", risk));
    }
    out.push_str("    }\n\n");

    out.push_str("    load_policy {\n");
    if let Some(mode) = &library.load_policy.default_context_mode {
        out.push_str(&format!("        default_context_mode: {};\n", mode));
    }
    if let Some(max) = library.load_policy.max_primary_subskills {
        out.push_str(&format!("        max_primary_subskills: {};\n", max));
    }
    if let Some(max) = library.load_policy.max_total_subskills {
        out.push_str(&format!("        max_total_subskills: {};\n", max));
    }
    if let Some(required) = library.load_policy.require_dependency_closure {
        out.push_str(&format!(
            "        require_dependency_closure: {};\n",
            required
        ));
    }
    if let Some(mode) = &library.load_policy.allow_extended_load {
        out.push_str(&format!("        allow_extended_load: {};\n", mode));
    }
    out.push_str("    }\n\n");

    for policy in library.policies.values() {
        out.push_str(&format!("    policy {} {{\n", policy.id));
        if let Some(inherits) = &policy.inherits {
            out.push_str(&format!("        inherits: {};\n", inherits));
        }
        render_string_list(&mut out, "allowed", &policy.allowed, 2, false);
        render_string_list(
            &mut out,
            "requires_approval",
            &policy.requires_approval,
            2,
            false,
        );
        render_string_list(&mut out, "forbidden", &policy.forbidden, 2, false);
        out.push_str("    }\n\n");
    }

    out.push_str("    index {\n");
    for item in library.index.values() {
        out.push_str(&format!("        item {} {{\n", item.id));
        if let Some(title) = &item.title {
            out.push_str(&format!(
                "            title: \"{}\";\n",
                escape_string(title)
            ));
        }
        if let Some(summary) = &item.summary {
            out.push_str(&format!(
                "            summary: \"{}\";\n",
                escape_string(summary)
            ));
        }
        render_string_list(&mut out, "tags", &item.tags, 3, false);
        if let Some(risk) = &item.risk {
            out.push_str(&format!("            risk: {};\n", risk));
        }
        if !item.token_cost.is_empty() {
            out.push_str("            token_cost {\n");
            for (key, value) in &item.token_cost {
                out.push_str(&format!("                {}: {};\n", key, value));
            }
            out.push_str("            }\n");
        }
        out.push_str("        }\n");
    }
    out.push_str("    }\n\n");

    for subskill in &library.subskills {
        render_subskill(&mut out, subskill);
    }
    for link in &library.links {
        render_link(&mut out, link);
    }
    for eval in &library.evals {
        render_eval(&mut out, eval);
    }
    out.push_str("}\n");
    Ok(out)
}

fn render_subskill(out: &mut String, subskill: &LslSubskill) {
    out.push_str(&format!("    subskill {} {{\n", subskill.id));
    out.push_str(&format!("        id: {};\n", subskill.id));
    if let Some(title) = &subskill.title {
        out.push_str(&format!("        title: \"{}\";\n", escape_string(title)));
    }
    if let Some(version) = &subskill.version {
        out.push_str(&format!(
            "        version: \"{}\";\n",
            escape_string(version)
        ));
    }
    if let Some(status) = &subskill.status {
        out.push_str(&format!("        status: {};\n", status));
    }
    if let Some(skill_type) = &subskill.skill_type {
        out.push_str(&format!("        type: {};\n", skill_type));
    }
    if let Some(risk) = &subskill.risk {
        out.push_str(&format!("        risk: {};\n", risk));
    }
    if let Some(summary) = &subskill.summary {
        out.push_str(&format!(
            "        summary: \"{}\";\n",
            escape_string(summary)
        ));
    }
    render_string_list(out, "tags", &subskill.tags, 2, false);
    if !subskill.activation_positive.is_empty() || !subskill.activation_negative.is_empty() {
        out.push_str("        activation {\n");
        render_string_list(out, "positive", &subskill.activation_positive, 3, true);
        render_string_list(out, "negative", &subskill.activation_negative, 3, true);
        out.push_str("        }\n");
    }
    out.push_str("        signature {\n");
    for input in &subskill.inputs {
        out.push_str(&format!(
            "            input {}: {} {};\n",
            input.name,
            input.field_type,
            if input.required {
                "required"
            } else {
                "optional"
            }
        ));
    }
    for output in &subskill.outputs {
        out.push_str(&format!(
            "            output {}: {};\n",
            output.name, output.field_type
        ));
    }
    out.push_str("        }\n");
    out.push_str("        requires {\n");
    render_string_list(out, "concepts", &subskill.required_concepts, 3, false);
    render_string_list(out, "tools", &subskill.required_tools, 3, false);
    out.push_str("        }\n");
    out.push_str("        policy {\n");
    if let Some(inherits) = &subskill.policy.inherits {
        out.push_str(&format!("            inherits: {};\n", inherits));
    }
    render_string_list(out, "allowed", &subskill.policy.allowed, 3, false);
    render_string_list(
        out,
        "requires_approval",
        &subskill.policy.requires_approval,
        3,
        false,
    );
    render_string_list(out, "forbidden", &subskill.policy.forbidden, 3, false);
    out.push_str("        }\n");
    out.push_str("        context_budget {\n");
    for (key, value) in &subskill.context_budget {
        out.push_str(&format!("            {}: {};\n", key, value));
    }
    out.push_str("        }\n");
    out.push_str("        load {\n");
    for key in ["card", "body", "extended"] {
        if let Some(value) = subskill.load.get(key) {
            out.push_str(&format!(
                "            {}: \"\"\"\n{}\n\"\"\";\n",
                key,
                value.trim()
            ));
        }
    }
    out.push_str("        }\n");
    render_string_list(out, "forbidden", &subskill.forbidden, 2, true);
    render_string_list(out, "verification", &subskill.verification, 2, true);
    render_string_list(out, "failure_modes", &subskill.failure_modes, 2, true);
    if !subskill.metrics.is_empty() {
        out.push_str("        metrics {\n");
        for (key, value) in &subskill.metrics {
            out.push_str(&format!("            {}: {};\n", key, render_value(value)));
        }
        out.push_str("        }\n");
    }
    render_string_list(out, "eval_refs", &subskill.eval_refs, 2, false);
    out.push_str("    }\n\n");
}

fn render_link(out: &mut String, link: &LslLink) {
    out.push_str(&format!("    link {} {{\n", link.id));
    out.push_str(&format!("        from: {};\n", link.from));
    out.push_str(&format!("        to: {};\n", link.to));
    if let Some(relation) = &link.relation {
        out.push_str(&format!("        relation: {};\n", relation));
    }
    if let Some(strength) = link.strength {
        out.push_str(&format!("        strength: {:.1};\n", strength));
    }
    if let Some(load_hint) = &link.load_hint {
        out.push_str(&format!("        load_hint: {};\n", load_hint));
    }
    out.push_str("    }\n\n");
}

fn render_eval(out: &mut String, eval: &LslEval) {
    out.push_str(&format!("    eval {} {{\n", eval.id));
    if let Some(target) = &eval.target {
        out.push_str(&format!("        target: {};\n", target));
    }
    if let Some(task) = &eval.task {
        out.push_str(&format!("        task: \"{}\";\n", escape_string(task)));
    }
    render_string_list(out, "expected", &eval.expected, 2, true);
    render_string_list(out, "forbidden", &eval.forbidden, 2, true);
    if !eval.scoring.is_empty() {
        out.push_str("        scoring {\n");
        for (key, value) in &eval.scoring {
            out.push_str(&format!("            {}: {:.2};\n", key, value));
        }
        out.push_str("        }\n");
    }
    out.push_str("    }\n\n");
}

fn render_string_list(out: &mut String, name: &str, values: &[String], indent: usize, quote: bool) {
    let prefix = "    ".repeat(indent);
    out.push_str(&format!("{prefix}{name}: ["));
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        if quote {
            out.push_str(&format!("\"{}\"", escape_string(value)));
        } else {
            out.push_str(value);
        }
    }
    out.push_str("];\n");
}

fn render_value(value: &Value) -> String {
    match value {
        Value::String(value) => format!("\"{}\"", escape_string(value)),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => format!("\"{}\"", escape_string(&value.to_string())),
    }
}

fn escape_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn find_first_block(source: &str, keyword: &str) -> anyhow::Result<Option<NamedBlock>> {
    Ok(named_blocks(source, keyword)?.into_iter().next())
}

fn top_level_blocks(source: &str) -> anyhow::Result<Vec<NamedBlock>> {
    let mut blocks = Vec::new();
    let mut cursor = 0;
    while let Some(start) = next_identifier(source, cursor) {
        let (keyword, after_keyword) = read_identifier(source, start);
        let mut at = skip_ws(source, after_keyword);
        let name = if byte_at(source, at) == Some(b'{') {
            None
        } else {
            let (name, next) = read_name(source, at);
            at = skip_ws(source, next);
            (!name.is_empty()).then_some(name)
        };
        if byte_at(source, at) != Some(b'{') {
            cursor = after_keyword;
            continue;
        }
        let end =
            matching_brace(source, at).with_context(|| format!("unterminated {keyword} block"))?;
        blocks.push(NamedBlock {
            keyword,
            name,
            body: source[at + 1..end].to_string(),
        });
        cursor = end + 1;
    }
    Ok(blocks)
}

fn named_blocks(source: &str, keyword: &str) -> anyhow::Result<Vec<NamedBlock>> {
    Ok(top_level_blocks(source)?
        .into_iter()
        .filter(|block| block.keyword == keyword)
        .collect())
}

fn nested_block(source: &str, keyword: &str) -> anyhow::Result<Option<String>> {
    Ok(top_level_blocks(source)?
        .into_iter()
        .find(|block| block.keyword == keyword)
        .map(|block| block.body))
}

fn parse_field_map(source: &str) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    for statement in source.split(';') {
        let Some((key, raw)) = statement.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = raw.trim();
        if key.is_empty() || value.is_empty() || value.contains('{') || value.contains('[') {
            continue;
        }
        fields.insert(key.to_string(), Value::String(unquote(value).to_string()));
    }
    fields
}

fn atom_field(source: &str, key: &str) -> Option<String> {
    field_raw(source, key).map(|value| unquote(value.trim()).to_string())
}

fn string_field(source: &str, key: &str) -> Option<String> {
    atom_field(source, key)
}

fn number_field(source: &str, key: &str) -> Option<f64> {
    atom_field(source, key).and_then(|value| value.parse::<f64>().ok())
}

fn usize_field(source: &str, key: &str) -> Option<usize> {
    atom_field(source, key).and_then(|value| value.parse::<usize>().ok())
}

fn bool_field(source: &str, key: &str) -> Option<bool> {
    atom_field(source, key).and_then(|value| match value.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn field_raw<'a>(source: &'a str, key: &str) -> Option<&'a str> {
    let mut cursor = 0;
    while let Some(found) = find_word(source, key, cursor) {
        let after_key = skip_ws(source, found + key.len());
        if byte_at(source, after_key) != Some(b':') {
            cursor = after_key;
            continue;
        }
        let value_start = skip_ws(source, after_key + 1);
        let end = statement_end(source, value_start)?;
        return Some(&source[value_start..end]);
    }
    None
}

fn list_field(source: &str, key: &str) -> Vec<String> {
    let Some(raw) = field_raw(source, key) else {
        return Vec::new();
    };
    parse_list(raw)
}

fn nested_list_field(source: &str, block: &str, key: &str) -> anyhow::Result<Vec<String>> {
    Ok(nested_block(source, block)?
        .map(|body| list_field(&body, key))
        .unwrap_or_default())
}

fn numeric_block(source: &str, block: &str) -> anyhow::Result<BTreeMap<String, u64>> {
    let Some(body) = nested_block(source, block)? else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for statement in body.split(';') {
        let Some((key, value)) = statement.split_once(':') else {
            continue;
        };
        if let Ok(value) = value.trim().parse::<u64>() {
            out.insert(key.trim().to_string(), value);
        }
    }
    Ok(out)
}

fn float_block(source: &str, block: &str) -> anyhow::Result<BTreeMap<String, f64>> {
    let Some(body) = nested_block(source, block)? else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for statement in body.split(';') {
        let Some((key, value)) = statement.split_once(':') else {
            continue;
        };
        if let Ok(value) = value.trim().parse::<f64>() {
            out.insert(key.trim().to_string(), value);
        }
    }
    Ok(out)
}

fn text_block(source: &str, block: &str) -> anyhow::Result<BTreeMap<String, String>> {
    let Some(body) = nested_block(source, block)? else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for key in ["card", "body", "extended"] {
        if let Some(value) = field_raw(&body, key) {
            out.insert(key.to_string(), unquote(value.trim()).trim().to_string());
        }
    }
    Ok(out)
}

fn nested_policy(source: &str) -> anyhow::Result<LslPolicy> {
    let Some(body) = nested_block(source, "policy")? else {
        return Ok(LslPolicy::default());
    };
    Ok(LslPolicy {
        id: String::new(),
        inherits: atom_field(&body, "inherits"),
        allowed: list_field(&body, "allowed"),
        requires_approval: list_field(&body, "requires_approval"),
        forbidden: list_field(&body, "forbidden"),
    })
}

fn signature_fields(source: &str, direction: &str) -> anyhow::Result<Vec<LslSignatureField>> {
    let Some(body) = nested_block(source, "signature")? else {
        return Ok(Vec::new());
    };
    let mut fields = Vec::new();
    for statement in body
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let Some(rest) = statement.strip_prefix(direction) else {
            continue;
        };
        let rest = rest.trim();
        let Some((name, after_name)) = rest.split_once(':') else {
            continue;
        };
        let parts = after_name.split_whitespace().collect::<Vec<_>>();
        fields.push(LslSignatureField {
            name: name.trim().to_string(),
            field_type: parts.first().copied().unwrap_or("unknown").to_string(),
            required: parts.contains(&"required"),
        });
    }
    Ok(fields)
}

fn nested_value_map(source: &str, block: &str) -> anyhow::Result<BTreeMap<String, Value>> {
    let Some(body) = nested_block(source, block)? else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for statement in body.split(';') {
        let Some((key, raw)) = statement.split_once(':') else {
            continue;
        };
        let raw = raw.trim();
        let value = if let Ok(number) = raw.parse::<u64>() {
            Value::Number(number.into())
        } else if let Ok(float) = raw.parse::<f64>() {
            json!(float)
        } else if raw == "true" || raw == "false" {
            Value::Bool(raw == "true")
        } else {
            Value::String(unquote(raw).to_string())
        };
        out.insert(key.trim().to_string(), value);
    }
    Ok(out)
}

fn canonical_lsl_value(library: &LinkedSkillLibrary) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(library)?;
    sort_json_value(&mut value);
    Ok(value)
}

fn sort_json_value(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                sort_json_value(item);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                sort_json_value(value);
            }
        }
        _ => {}
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn collect_lsl_files(
    root: &Path,
    visit: &mut impl FnMut(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(root)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            collect_lsl_files(&path, visit)?;
            continue;
        }
        if is_lsl_file(&path) {
            visit(&path)?;
        }
    }
    Ok(())
}

fn collect_lsl_file_paths(roots: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        collect_lsl_files(root, &mut |path| {
            paths.push(path.to_path_buf());
            Ok(())
        })?;
    }
    Ok(paths)
}

fn compile_lsl_file(path: &Path) -> anyhow::Result<(LinkedSkillLibrary, LslHashes)> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read LSL file {}", path.display()))?;
    let library = parse_lsl(&source)
        .with_context(|| format!("failed to compile LSL file {}", path.display()))?;
    let hashes = LslHashes {
        source_path: path.display().to_string(),
        source_hash: library.source_hash.clone().unwrap_or_default(),
        canonical_hash: library.canonical_hash.clone().unwrap_or_default(),
        semantic_hash: library.semantic_hash.clone().unwrap_or_default(),
    };
    Ok((library, hashes))
}

fn is_lsl_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("lsl"))
        .unwrap_or(false)
}

fn collect_lsl_source_hashes(roots: &[PathBuf]) -> anyhow::Result<BTreeMap<String, String>> {
    let paths = collect_lsl_file_paths(roots)?;
    let workers = ParallelismConfig::detect().constrained_workers(paths.len());
    let source_hashes = run_parallel_ordered(paths, workers, |path| {
        let source = fs::read(&path)
            .with_context(|| format!("failed to read LSL source {}", path.display()))?;
        Ok::<_, anyhow::Error>((path.display().to_string(), sha256_hex(&source)))
    });

    let mut hashes = BTreeMap::new();
    for source_hash in source_hashes {
        let (path, hash) = source_hash?;
        hashes.insert(path, hash);
    }
    Ok(hashes)
}

fn write_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    fs::write(path, serde_json::to_string_pretty(value)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> anyhow::Result<T> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read compiled LSL artifact {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse compiled LSL artifact {}", path.display()))
}

fn parse_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Vec::new();
    }
    let inner = &trimmed[1..trimmed.len().saturating_sub(1)];
    split_list_items(inner)
        .into_iter()
        .map(|item| unquote(item.trim()).to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn split_list_items(source: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut cursor = 0;
    let mut in_string = false;
    let bytes = source.as_bytes();
    while cursor < bytes.len() {
        if source[cursor..].starts_with("\"\"\"") {
            in_string = !in_string;
            cursor += 3;
            continue;
        }
        if bytes[cursor] == b'"' {
            in_string = !in_string;
        } else if bytes[cursor] == b',' && !in_string {
            items.push(source[start..cursor].trim());
            start = cursor + 1;
        }
        cursor += 1;
    }
    items.push(source[start..].trim());
    items
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    if value.starts_with("\"\"\"") && value.ends_with("\"\"\"") && value.len() >= 6 {
        &value[3..value.len() - 3]
    } else if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn statement_end(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let bytes = source.as_bytes();
    while cursor < bytes.len() {
        if source[cursor..].starts_with("\"\"\"") {
            in_string = !in_string;
            cursor += 3;
            continue;
        }
        if bytes[cursor] == b'"' {
            in_string = !in_string;
        }
        if !in_string {
            match bytes[cursor] {
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b';' if brace_depth == 0 && bracket_depth == 0 => return Some(cursor),
                _ => {}
            }
        }
        cursor += 1;
    }
    None
}

fn matching_brace(source: &str, open: usize) -> Option<usize> {
    let mut cursor = open + 1;
    let mut depth = 1usize;
    let mut in_string = false;
    let bytes = source.as_bytes();
    while cursor < bytes.len() {
        if source[cursor..].starts_with("\"\"\"") {
            in_string = !in_string;
            cursor += 3;
            continue;
        }
        if bytes[cursor] == b'"' {
            in_string = !in_string;
        }
        if !in_string {
            match bytes[cursor] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(cursor);
                    }
                }
                _ => {}
            }
        }
        cursor += 1;
    }
    None
}

fn next_identifier(source: &str, start: usize) -> Option<usize> {
    source[start..]
        .char_indices()
        .find(|(_, ch)| ch.is_ascii_alphabetic() || *ch == '_')
        .map(|(offset, _)| start + offset)
}

fn read_identifier(source: &str, start: usize) -> (String, usize) {
    let end = source[start..]
        .char_indices()
        .find(|(_, ch)| !(ch.is_ascii_alphanumeric() || *ch == '_'))
        .map(|(offset, _)| start + offset)
        .unwrap_or(source.len());
    (source[start..end].to_string(), end)
}

fn read_name(source: &str, start: usize) -> (String, usize) {
    let end = source[start..]
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace() || *ch == '{')
        .map(|(offset, _)| start + offset)
        .unwrap_or(source.len());
    (source[start..end].trim().to_string(), end)
}

fn skip_ws(source: &str, start: usize) -> usize {
    source[start..]
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(offset, _)| start + offset)
        .unwrap_or(source.len())
}

fn byte_at(source: &str, index: usize) -> Option<u8> {
    source.as_bytes().get(index).copied()
}

fn find_word(source: &str, word: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    while let Some(offset) = source[cursor..].find(word) {
        let found = cursor + offset;
        let before = found
            .checked_sub(1)
            .and_then(|index| source.as_bytes().get(index))
            .copied();
        let after = source.as_bytes().get(found + word.len()).copied();
        let boundary_before = before
            .map(|byte| !(byte.is_ascii_alphanumeric() || byte == b'_'))
            .unwrap_or(true);
        let boundary_after = after
            .map(|byte| !(byte.is_ascii_alphanumeric() || byte == b'_'))
            .unwrap_or(true);
        if boundary_before && boundary_after {
            return Some(found);
        }
        cursor = found + word.len();
    }
    None
}

fn json_string_array(values: Vec<String>) -> Value {
    Value::Array(values.into_iter().map(Value::String).collect())
}

#[cfg(test)]
mod tests {
    use super::{LslRegistry, load_compiled_registry, lsl_registry_status, parse_lsl};
    use serde_json::Value;

    #[test]
    fn parses_library_subskills_and_links() {
        let parsed = parse_lsl(
            r#"
            library cryptography {
                meta {
                    id: "cryptography";
                    name: "Cryptography";
                    version: "1.0.0";
                    status: active;
                    risk: high;
                }

                index {
                    item cryptography.aes_256 {
                        title: "AES-256";
                        summary: "AES guidance.";
                        tags: [aes, "aes-gcm"];
                        risk: medium;
                        token_cost { card: 80; body: 700; }
                    }
                }

                subskill cryptography.secure_randomness {
                    id: cryptography.secure_randomness;
                    title: "Secure Randomness";
                    summary: "Use a CSPRNG.";
                    tags: [rng, entropy];
                    load {
                        card: """CSPRNG card.""";
                        body: """CSPRNG body.""";
                    }
                }

                subskill cryptography.aes_256 {
                    id: cryptography.aes_256;
                    title: "AES-256";
                    summary: "AES guidance.";
                    activation { positive: ["AES"]; negative: ["crack AES"]; }
                    requires { concepts: [cryptography.secure_randomness]; tools: []; }
                    context_budget { card_tokens: 80; body_tokens: 700; }
                    load {
                        card: """AES card.""";
                        body: """AES body.""";
                    }
                    forbidden: ["Do not crack AES."];
                    verification: ["Nonce handling is safe."];
                    eval_refs: [cryptography.aes_256.eval.design_review_001];
                }

                link cryptography.aes_256.requires.secure_randomness {
                    from: cryptography.aes_256;
                    to: cryptography.secure_randomness;
                    relation: requires;
                    strength: 1.0;
                    load_hint: card;
                }

                eval cryptography.aes_256.eval.design_review_001 {
                    target: cryptography.aes_256;
                    task: "Review AES.";
                    expected: ["checks nonce"];
                    forbidden: ["suggests ECB"];
                    scoring { correctness: 0.5; safety: 0.5; }
                }
            }
            "#,
        )
        .expect("valid lsl");

        assert_eq!(parsed.id, "cryptography");
        assert_eq!(parsed.name, "Cryptography");
        assert_eq!(parsed.subskills.len(), 2);
        assert_eq!(parsed.links.len(), 1);
        assert_eq!(parsed.evals.len(), 1);
        let aes = parsed
            .subskills
            .iter()
            .find(|subskill| subskill.id == "cryptography.aes_256")
            .unwrap();
        assert_eq!(
            aes.required_concepts,
            vec!["cryptography.secure_randomness"]
        );
        assert_eq!(aes.load.get("body").unwrap(), "AES body.");
    }

    #[test]
    fn rejects_duplicate_subskills() {
        let err = parse_lsl(
            r#"
            library demo {
                subskill demo.one {
                    id: demo.one;
                    title: "One";
                    summary: "First.";
                    load { card: """one"""; }
                }
                subskill demo.one {
                    id: demo.one;
                    title: "One";
                    summary: "Second.";
                    load { card: """two"""; }
                }
            }
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("duplicate subskill id"));
    }

    #[test]
    fn lsl_file_collection_is_deterministically_sorted() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let skills = tmp.path().join("skills");
        let nested = skills.join("nested");
        std::fs::create_dir_all(&nested)?;
        std::fs::write(skills.join("zeta.lsl"), "library zeta {}")?;
        std::fs::write(skills.join("alpha.txt"), "not an lsl file")?;
        std::fs::write(nested.join("alpha.lsl"), "library alpha {}")?;

        let paths = super::collect_lsl_file_paths(&[skills])?;
        let rendered = paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert_eq!(rendered, vec!["alpha.lsl", "zeta.lsl"]);
        Ok(())
    }

    #[test]
    fn compiles_registry_artifacts_and_hashes() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let skills = tmp.path().join("skills");
        let compiled = tmp.path().join(".vegvisir").join("compiled");
        std::fs::create_dir_all(&skills)?;
        std::fs::write(
            skills.join("cryptography.lsl"),
            r#"
            library cryptography {
                meta { id: "cryptography"; name: "Cryptography"; version: "1.0.0"; }
                load_policy {
                    default_context_mode: index_only;
                    max_primary_subskills: 3;
                    max_total_subskills: 8;
                    require_dependency_closure: true;
                    allow_extended_load: conditional;
                }
                policy cryptography.policy.default {
                    allowed: [education];
                    requires_approval: [live_wallet_handling];
                    forbidden: [wallet_theft];
                }
                subskill cryptography.secure_randomness {
                    id: cryptography.secure_randomness;
                    title: "Secure Randomness";
                    summary: "CSPRNG guidance.";
                    signature {
                        input task: string required;
                        output guidance: checklist;
                    }
                    policy { inherits: cryptography.policy.default; forbidden: [rng_prediction]; }
                    load { card: """Use a CSPRNG."""; body: """Use OS crypto randomness."""; }
                    metrics { use_count: 0; eval_score: 0.0; }
                }
            }
            "#,
        )?;

        let result = super::compile_lsl_roots(&[skills], &compiled)?;
        let loaded = load_compiled_registry(&compiled)?;

        assert_eq!(result.registry.libraries.len(), 1);
        assert_eq!(loaded.registry.subskills.len(), 1);
        assert_eq!(
            result.hashes["cryptography"].source_path,
            tmp.path()
                .join("skills")
                .join("cryptography.lsl")
                .display()
                .to_string()
        );
        assert!(
            result.hashes["cryptography"]
                .source_hash
                .starts_with("sha256:")
        );
        assert!(compiled.join("index").join("subskills.json").exists());
        assert!(compiled.join("hashes").join("hashes.json").exists());
        let entry = result
            .registry
            .subskills
            .get("cryptography.secure_randomness")
            .unwrap();
        assert_eq!(entry.subskill.inputs[0].name, "task");
        assert_eq!(
            entry.subskill.policy.inherits.as_deref(),
            Some("cryptography.policy.default")
        );
        Ok(())
    }

    #[test]
    fn registry_status_detects_stale_sources() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let skills = tmp.path().join("skills");
        let compiled = tmp.path().join(".vegvisir").join("compiled");
        std::fs::create_dir_all(&skills)?;
        let path = skills.join("cryptography.lsl");
        std::fs::write(
            &path,
            r#"
            library cryptography {
                subskill cryptography.secure_randomness {
                    id: cryptography.secure_randomness;
                    title: "Secure Randomness";
                    summary: "CSPRNG guidance.";
                    load { card: """Use a CSPRNG."""; }
                }
            }
            "#,
        )?;
        super::compile_lsl_roots(std::slice::from_ref(&skills), &compiled)?;
        let fresh = lsl_registry_status(std::slice::from_ref(&skills), &compiled)?;
        assert!(fresh.fresh);

        std::fs::write(
            &path,
            r#"
            library cryptography {
                subskill cryptography.secure_randomness {
                    id: cryptography.secure_randomness;
                    title: "Secure Randomness";
                    summary: "Updated CSPRNG guidance.";
                    load { card: """Use operating-system randomness."""; }
                }
            }
            "#,
        )?;
        let stale = lsl_registry_status(&[skills], &compiled)?;
        assert!(!stale.fresh);
        assert_eq!(stale.stale_sources, vec![path.display().to_string()]);
        Ok(())
    }

    #[test]
    fn eval_hooks_report_pass_and_failure() -> anyhow::Result<()> {
        let parsed = parse_lsl(
            r#"
            library cryptography {
                subskill cryptography.secure_randomness {
                    id: cryptography.secure_randomness;
                    title: "Secure Randomness";
                    summary: "CSPRNG guidance.";
                    load { card: """Use a CSPRNG, never math.random."""; }
                    eval_refs: [cryptography.secure_randomness.eval.basic];
                }
                eval cryptography.secure_randomness.eval.basic {
                    target: cryptography.secure_randomness;
                    task: "Check randomness guidance.";
                    expected: ["CSPRNG"];
                    forbidden: ["math.random"];
                    scoring { correctness: 0.5; safety: 0.5; }
                }
            }
            "#,
        )?;
        let registry = LslRegistry::from_libraries(vec![parsed]);
        let reports = registry.eval_hooks(None);

        assert_eq!(reports.len(), 1);
        assert!(!reports[0].passed);
        assert_eq!(reports[0].score, 0.5);
        assert_eq!(reports[0].present_forbidden, vec!["math.random"]);
        Ok(())
    }

    #[test]
    fn rejects_invalid_enums_and_scoring_weights() {
        let err = parse_lsl(
            r#"
            library demo {
                subskill demo.one {
                    id: demo.one;
                    title: "One";
                    summary: "Invalid.";
                    type: improvisation;
                    risk: medium;
                    load { card: """one"""; }
                }
            }
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid value 'improvisation'"));

        let err = parse_lsl(
            r#"
            library demo {
                subskill demo.one {
                    id: demo.one;
                    title: "One";
                    summary: "Invalid eval.";
                    load { card: """one"""; }
                    eval_refs: [demo.one.eval.bad];
                }
                eval demo.one.eval.bad {
                    target: demo.one;
                    expected: ["one"];
                    scoring { correctness: 0.9; safety: 0.9; }
                }
            }
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("scoring weights must sum to 1.0"));
    }

    #[test]
    fn bundled_example_library_parses() {
        let parsed = parse_lsl(include_str!("defaults/example_cryptography.lsl"))
            .expect("bundled example should stay valid");
        assert_eq!(parsed.id, "cryptography");
        assert_eq!(parsed.subskills.len(), 2);
        assert_eq!(parsed.links.len(), 1);
        assert_eq!(parsed.evals.len(), 2);
    }

    #[test]
    fn forges_candidate_subskill_and_promotes_status() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("skills");
        let path = super::forge_candidate_subskill(
            &root,
            super::LslSkillDraft {
                id: "software_engineering.rust_nextest".to_string(),
                title: "Rust Nextest".to_string(),
                summary: "Use cargo nextest for Rust test execution.".to_string(),
                body: "Run cargo nextest and inspect failing test output.".to_string(),
                provenance: "unit-test trace".to_string(),
                tags: vec!["rust".to_string(), "nextest".to_string()],
            },
        )?;
        let source = std::fs::read_to_string(&path)?;
        let parsed = parse_lsl(&source)?;
        assert_eq!(parsed.subskills[0].status.as_deref(), Some("candidate"));
        assert_eq!(
            parsed.subskills[0]
                .metrics
                .get("provenance")
                .and_then(Value::as_str),
            Some("unit-test trace")
        );

        super::set_subskill_status(&root, "software_engineering.rust_nextest", "active")?;
        let parsed = parse_lsl(&std::fs::read_to_string(&path)?)?;
        assert_eq!(parsed.subskills[0].status.as_deref(), Some("active"));
        Ok(())
    }

    #[test]
    fn registry_routes_and_loads_dependency_closure() {
        let parsed = parse_lsl(
            r#"
            library cryptography {
                index {
                    item cryptography.aes_256 {
                        title: "AES-256";
                        summary: "AES-GCM encryption guidance.";
                        tags: [aes, encryption];
                    }
                    item cryptography.secure_randomness {
                        title: "Secure Randomness";
                        summary: "CSPRNG nonce entropy guidance.";
                        tags: [rng, nonce];
                    }
                }
                subskill cryptography.secure_randomness {
                    id: cryptography.secure_randomness;
                    title: "Secure Randomness";
                    summary: "Use CSPRNGs.";
                    load { card: """Use a CSPRNG."""; body: """Use OS randomness."""; }
                }
                subskill cryptography.aes_256 {
                    id: cryptography.aes_256;
                    title: "AES-256";
                    summary: "AES-GCM encryption guidance.";
                    tags: [aes, encryption];
                    requires { concepts: [cryptography.secure_randomness]; tools: []; }
                    load { card: """AES card."""; body: """AES body."""; }
                    verification: ["Nonce uniqueness is checked."];
                }
                link cryptography.aes_256.requires.secure_randomness {
                    from: cryptography.aes_256;
                    to: cryptography.secure_randomness;
                    relation: requires;
                    load_hint: card;
                }
            }
            "#,
        )
        .expect("valid lsl");
        let registry = LslRegistry::from_libraries(vec![parsed]);

        assert!(registry.issues.is_empty());
        assert_eq!(
            registry.route("review AES-GCM encryption nonce handling", 1),
            vec!["cryptography.aes_256"]
        );

        let context = registry.load_context(&["cryptography.aes_256".to_string()], 200, 2);
        assert_eq!(context.selected.len(), 2);
        assert_eq!(context.selected[0].id, "cryptography.aes_256");
        assert_eq!(context.selected[0].mode, "body");
        assert_eq!(context.selected[1].id, "cryptography.secure_randomness");
        assert_eq!(context.selected[1].mode, "card");
    }

    #[test]
    fn patches_traces_curates_and_detects_missing_skills() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("skills");
        super::forge_candidate_subskill(
            &root,
            super::LslSkillDraft {
                id: "software_engineering.rust_nextest".to_string(),
                title: "Rust Nextest".to_string(),
                summary: "Use cargo nextest for Rust test execution.".to_string(),
                body: "Run cargo nextest.".to_string(),
                provenance: "unit-test trace".to_string(),
                tags: vec!["rust".to_string()],
            },
        )?;
        super::patch_subskill(
            &root,
            super::LslPatchRequest {
                target: "software_engineering.rust_nextest".to_string(),
                operation: "append_list_items".to_string(),
                path: "verification".to_string(),
                value: "Nextest output is inspected".to_string(),
            },
        )?;
        let parsed = parse_lsl(&std::fs::read_to_string(
            root.join("software_engineering.lsl"),
        )?)?;
        assert!(
            parsed.subskills[0]
                .verification
                .contains(&"Nextest output is inspected".to_string())
        );

        let trace_path = tmp.path().join("skill_traces.json");
        for _ in 0..2 {
            super::append_skill_trace(
                &trace_path,
                super::LslSkillTrace {
                    event: "route".to_string(),
                    query: "unknown niche build tool".to_string(),
                    selected: Vec::new(),
                    token_estimate: 0,
                    created_at: "now".to_string(),
                    ..super::LslSkillTrace::default()
                },
            )?;
        }
        let traces = super::read_skill_traces(&trace_path)?;
        assert_eq!(traces.len(), 2);
        assert!(!super::detect_missing_skill_candidates(&traces).is_empty());

        let registry = LslRegistry::from_libraries(vec![parsed]);
        let report = super::curate_registry(&registry, &traces, &registry.eval_hooks(None));
        assert_eq!(report.total_subskills, 1);
        assert_eq!(
            report.candidate_subskills,
            vec!["software_engineering.rust_nextest"]
        );
        assert!(!report.missing_skill_candidates.is_empty());
        Ok(())
    }
    #[test]
    fn dependency_plan_prefers_replacements_and_policy_fallbacks() {
        let parsed = parse_lsl(
            r#"
            library demo {
                subskill demo.old {
                    id: demo.old;
                    title: "Old";
                    summary: "Old guidance.";
                    status: stale;
                    load { card: """old"""; body: """old body"""; }
                }
                subskill demo.new {
                    id: demo.new;
                    title: "New";
                    summary: "Replacement guidance.";
                    status: active;
                    load { card: """new"""; body: """new body"""; }
                }
                subskill demo.blocked {
                    id: demo.blocked;
                    title: "Blocked";
                    summary: "Unsafe wallet handling.";
                    policy { forbidden: [wallet_theft]; }
                    load { card: """blocked"""; body: """blocked body"""; }
                }
                subskill demo.safe {
                    id: demo.safe;
                    title: "Safe";
                    summary: "Safe wallet alternative.";
                    load { card: """safe"""; body: """safe body"""; }
                }
                link demo.new.replaces.old {
                    from: demo.new;
                    to: demo.old;
                    relation: replaces;
                }
                link demo.blocked.fallback.safe {
                    from: demo.blocked;
                    to: demo.safe;
                    relation: fallback;
                }
            }
            "#,
        )
        .expect("valid lsl");
        let registry = LslRegistry::from_libraries(vec![parsed]);

        let replacement =
            registry.load_context_for_query(&["demo.old".to_string()], "old guidance", 200, 1);
        assert_eq!(replacement.selected[0].id, "demo.new");
        assert!(
            replacement
                .excluded
                .iter()
                .any(|item| item.id == "demo.old")
        );

        let fallback =
            registry.load_context_for_query(&["demo.blocked".to_string()], "wallet_theft", 200, 1);
        assert_eq!(fallback.selected[0].id, "demo.safe");
        assert!(
            fallback
                .excluded
                .iter()
                .any(|item| item.id == "demo.blocked")
        );
    }
}
