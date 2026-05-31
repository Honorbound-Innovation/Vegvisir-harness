use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const AUTONOMY_COMPILER_VERSION: &str = "autonomy-cll-pll-compiler-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutonomyPlanDocument {
    pub objective: String,
    pub nodes: Vec<AutonomyPlanNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutonomyPlanNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub level: usize,
    pub ordinal_path: Vec<usize>,
    pub title: String,
    pub body: Vec<String>,
    pub checklist: Vec<AutonomyChecklistItem>,
    pub success_conditions: Vec<String>,
    pub expected_deliverables: Vec<String>,
    pub implementation_rules: Vec<String>,
    pub guardrails: Vec<String>,
    pub validation: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutonomyChecklistItem {
    pub text: String,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutonomyCompiledLibraries {
    pub cll: String,
    pub pll: String,
    pub manifest: String,
    pub source_hash: String,
    pub cll_hash: String,
    pub pll_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutonomyLibraryPaths {
    pub cll_path: PathBuf,
    pub pll_path: PathBuf,
    pub manifest_path: PathBuf,
    pub state_path: PathBuf,
    pub evidence_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AutonomyPlanStatus {
    pub total_nodes: usize,
    pub completed_nodes: usize,
    pub current_node_id: Option<String>,
    pub current_node_title: Option<String>,
    pub current_node_index: Option<usize>,
    pub evidence_dir: Option<String>,
    pub current_evidence_path: Option<String>,
    pub current_evidence_valid: bool,
    pub current_evidence_errors: Vec<String>,
    pub nodes: Vec<AutonomyNodeStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AutonomyNodeStatus {
    pub id: String,
    pub parent_id: Option<String>,
    pub level: usize,
    pub title: String,
    pub checklist_total: usize,
    pub checklist_completed: usize,
    pub checklist_complete: bool,
    pub evidence_path: Option<String>,
    pub evidence_valid: bool,
    pub evidence_errors: Vec<String>,
    pub complete: bool,
    pub success_conditions: Vec<String>,
    pub expected_deliverables: Vec<String>,
    pub implementation_rules: Vec<String>,
    pub guardrails: Vec<String>,
    pub validation: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AutonomyCompletionPacket {
    pub node_id: String,
    pub status: String,
    #[serde(default)]
    pub actions_taken: Vec<String>,
    #[serde(default)]
    pub deliverables: Vec<AutonomyEvidenceItem>,
    #[serde(default)]
    pub success_conditions_satisfied: Vec<AutonomyConditionEvidence>,
    #[serde(default)]
    pub verification: Vec<AutonomyVerificationEvidence>,
    #[serde(default)]
    pub risks_or_blockers: Vec<String>,
    pub next_recommended_action: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AutonomyEvidenceItem {
    #[serde(rename = "type")]
    pub kind: String,
    pub path: Option<String>,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AutonomyConditionEvidence {
    pub condition: String,
    pub evidence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AutonomyVerificationEvidence {
    pub command: Option<String>,
    pub result: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AutonomyEvidenceValidation {
    pub node_id: String,
    pub evidence_path: String,
    pub valid: bool,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct AutonomyCompileManifest<'a> {
    compiler_version: &'a str,
    compiled_at: String,
    objective: &'a str,
    source: &'a str,
    cll: &'a str,
    pll: &'a str,
    source_hash: &'a str,
    cll_hash: &'a str,
    pll_hash: &'a str,
    node_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SemanticList {
    SuccessConditions,
    ExpectedDeliverables,
    ImplementationRules,
    Guardrails,
    Validation,
}

pub(crate) fn parse_autonomy_markdown_plan(
    markdown: &str,
    objective: &str,
) -> AutonomyPlanDocument {
    let mut nodes = Vec::<AutonomyPlanNode>::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut ordinals = [0usize; 6];
    let mut current: Option<AutonomyPlanNode> = None;
    let mut semantic_list: Option<SemanticList> = None;

    for line in markdown.lines() {
        if let Some((level, title)) = parse_markdown_heading(line) {
            if let Some(node) = current.take() {
                nodes.push(node);
            }
            let depth = level.saturating_sub(1).min(ordinals.len() - 1);
            ordinals[depth] = ordinals[depth].saturating_add(1);
            for ordinal in ordinals.iter_mut().skip(depth + 1) {
                *ordinal = 0;
            }
            stack.retain(|(stack_level, _)| *stack_level < level);
            let ordinal_path = ordinals
                .iter()
                .take(depth + 1)
                .copied()
                .filter(|ordinal| *ordinal > 0)
                .collect::<Vec<_>>();
            let parent_id = stack.last().map(|(_, id)| id.clone());
            let id = node_id_from_title(&ordinal_path, &title);
            stack.push((level, id.clone()));
            current = Some(AutonomyPlanNode {
                id,
                parent_id,
                level,
                ordinal_path,
                title,
                body: Vec::new(),
                checklist: Vec::new(),
                success_conditions: Vec::new(),
                expected_deliverables: Vec::new(),
                implementation_rules: Vec::new(),
                guardrails: Vec::new(),
                validation: Vec::new(),
            });
            semantic_list = None;
            continue;
        }

        let Some(node) = current.as_mut() else {
            continue;
        };

        if let Some(section) = parse_semantic_label(line) {
            semantic_list = Some(section);
            node.body.push(line.to_string());
            continue;
        }

        if let Some((checked, text)) = parse_task_item(line) {
            node.checklist.push(AutonomyChecklistItem { text, checked });
            semantic_list = None;
            continue;
        }

        if let Some(item) = parse_plain_list_item(line) {
            if let Some(section) = semantic_list {
                match section {
                    SemanticList::SuccessConditions => node.success_conditions.push(item),
                    SemanticList::ExpectedDeliverables => node.expected_deliverables.push(item),
                    SemanticList::ImplementationRules => node.implementation_rules.push(item),
                    SemanticList::Guardrails => node.guardrails.push(item),
                    SemanticList::Validation => node.validation.push(item),
                }
                continue;
            }
        }

        if !line.trim().is_empty() {
            node.body.push(line.to_string());
        }
    }

    if let Some(node) = current.take() {
        nodes.push(node);
    }

    if nodes.is_empty() {
        nodes.push(AutonomyPlanNode {
            id: "phase_01_autonomous_objective".to_string(),
            parent_id: None,
            level: 1,
            ordinal_path: vec![1],
            title: "Autonomous Objective".to_string(),
            body: markdown.lines().map(ToString::to_string).collect(),
            checklist: Vec::new(),
            success_conditions: Vec::new(),
            expected_deliverables: Vec::new(),
            implementation_rules: Vec::new(),
            guardrails: Vec::new(),
            validation: Vec::new(),
        });
    }

    AutonomyPlanDocument {
        objective: objective.trim().to_string(),
        nodes,
    }
}

pub(crate) fn compile_autonomy_plan_libraries(
    markdown: &str,
    objective: &str,
    run_id: &str,
    source_path: &str,
    cll_path: &str,
    pll_path: &str,
) -> anyhow::Result<AutonomyCompiledLibraries> {
    let plan = parse_autonomy_markdown_plan(markdown, objective);
    let source_hash = sha256_hex(markdown);
    let cll = render_cll(&plan, run_id, source_path, pll_path);
    let pll = render_pll(&plan, run_id, cll_path);
    let cll_hash = sha256_hex(&cll);
    let pll_hash = sha256_hex(&pll);
    let manifest = serde_json::to_string_pretty(&AutonomyCompileManifest {
        compiler_version: AUTONOMY_COMPILER_VERSION,
        compiled_at: Utc::now().to_rfc3339(),
        objective: &plan.objective,
        source: source_path,
        cll: cll_path,
        pll: pll_path,
        source_hash: &source_hash,
        cll_hash: &cll_hash,
        pll_hash: &pll_hash,
        node_count: plan.nodes.len(),
    })?;
    Ok(AutonomyCompiledLibraries {
        cll,
        pll,
        manifest,
        source_hash,
        cll_hash,
        pll_hash,
    })
}

pub(crate) fn autonomy_library_paths_for_plan(plan_path: &Path) -> AutonomyLibraryPaths {
    let stem = plan_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("autonomy-plan");
    let parent = plan_path.parent().unwrap_or_else(|| Path::new(""));
    AutonomyLibraryPaths {
        cll_path: parent.join(format!("{stem}.cll")),
        pll_path: parent.join(format!("{stem}.pll")),
        manifest_path: parent.join(format!("{stem}-compile-manifest.json")),
        state_path: parent.join(format!("{stem}-state.json")),
        evidence_dir: parent.join(format!("{stem}-evidence")),
    }
}

pub(crate) fn write_autonomy_libraries(
    cwd: &Path,
    plan_path: &Path,
    objective: &str,
    run_id: &str,
) -> anyhow::Result<AutonomyLibraryPaths> {
    let absolute_plan_path = cwd.join(plan_path);
    let markdown = std::fs::read_to_string(&absolute_plan_path)?;
    let paths = autonomy_library_paths_for_plan(plan_path);
    let cll_path_text = paths.cll_path.display().to_string();
    let pll_path_text = paths.pll_path.display().to_string();
    let source_path_text = plan_path.display().to_string();
    let compiled = compile_autonomy_plan_libraries(
        &markdown,
        objective,
        run_id,
        &source_path_text,
        &cll_path_text,
        &pll_path_text,
    )?;

    if let Some(parent) = cwd.join(&paths.cll_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(cwd.join(&paths.cll_path), compiled.cll)?;
    std::fs::write(cwd.join(&paths.pll_path), compiled.pll)?;
    std::fs::write(cwd.join(&paths.manifest_path), compiled.manifest)?;
    std::fs::create_dir_all(cwd.join(&paths.evidence_dir))?;
    let status =
        autonomy_plan_status_with_evidence(&markdown, objective, Some(&paths.evidence_dir), cwd)?;
    write_current_evidence_template(cwd, &paths.evidence_dir, &status)?;
    std::fs::write(
        cwd.join(&paths.state_path),
        serde_json::to_string_pretty(&status)?,
    )?;
    Ok(paths)
}

pub(crate) fn autonomy_plan_status_with_evidence(
    markdown: &str,
    objective: &str,
    evidence_dir: Option<&Path>,
    cwd: &Path,
) -> anyhow::Result<AutonomyPlanStatus> {
    let provisional =
        autonomy_plan_status_unchecked(markdown, objective, evidence_dir, None, false, Vec::new());
    let mut nodes = provisional.nodes;
    if let Some(dir) = evidence_dir {
        for index in 0..nodes.len() {
            if nodes[index].checklist_total == 0 {
                continue;
            }
            let node_id = nodes[index].id.clone();
            let validation = validate_node_evidence(cwd, dir, &node_id, &nodes)?;
            nodes[index].evidence_path = Some(validation.evidence_path.clone());
            nodes[index].evidence_valid = validation.valid;
            nodes[index].evidence_errors = validation.errors;
            nodes[index].complete = nodes[index].checklist_complete && nodes[index].evidence_valid;
        }
    } else {
        for node in nodes.iter_mut() {
            node.complete = node.checklist_complete;
        }
    }

    let current_node_id = nodes
        .iter()
        .find(|node| node.checklist_total > 0 && !node.complete)
        .or_else(|| nodes.iter().find(|node| node.checklist_total > 0))
        .or_else(|| nodes.first())
        .map(|node| node.id.clone());
    let current_evidence_path = current_node_id
        .as_deref()
        .and_then(|node_id| evidence_dir.map(|dir| evidence_packet_path(dir, node_id)))
        .map(|path| path.display().to_string());
    let (current_evidence_valid, current_evidence_errors) = current_node_id
        .as_deref()
        .and_then(|node_id| nodes.iter().find(|node| node.id == node_id))
        .map(|node| (node.evidence_valid, node.evidence_errors.clone()))
        .unwrap_or((false, Vec::new()));

    Ok(autonomy_plan_status_from_nodes(
        nodes,
        evidence_dir,
        current_evidence_path,
        current_evidence_valid,
        current_evidence_errors,
    ))
}

fn autonomy_plan_status_unchecked(
    markdown: &str,
    objective: &str,
    evidence_dir: Option<&Path>,
    current_evidence_path: Option<String>,
    current_evidence_valid: bool,
    current_evidence_errors: Vec<String>,
) -> AutonomyPlanStatus {
    let plan = parse_autonomy_markdown_plan(markdown, objective);
    let nodes = plan
        .nodes
        .iter()
        .map(|node| {
            let checklist_total = node.checklist.len();
            let checklist_completed = node.checklist.iter().filter(|item| item.checked).count();
            let checklist_complete = checklist_total > 0 && checklist_completed == checklist_total;
            AutonomyNodeStatus {
                id: node.id.clone(),
                parent_id: node.parent_id.clone(),
                level: node.level,
                title: node.title.clone(),
                checklist_total,
                checklist_completed,
                checklist_complete,
                evidence_path: evidence_dir
                    .map(|dir| evidence_packet_path(dir, &node.id).display().to_string()),
                evidence_valid: false,
                evidence_errors: Vec::new(),
                complete: false,
                success_conditions: node.success_conditions.clone(),
                expected_deliverables: node.expected_deliverables.clone(),
                implementation_rules: node.implementation_rules.clone(),
                guardrails: node.guardrails.clone(),
                validation: node.validation.clone(),
            }
        })
        .collect::<Vec<_>>();
    autonomy_plan_status_from_nodes(
        nodes,
        evidence_dir,
        current_evidence_path,
        current_evidence_valid,
        current_evidence_errors,
    )
}

fn autonomy_plan_status_from_nodes(
    nodes: Vec<AutonomyNodeStatus>,
    evidence_dir: Option<&Path>,
    current_evidence_path: Option<String>,
    current_evidence_valid: bool,
    current_evidence_errors: Vec<String>,
) -> AutonomyPlanStatus {
    let executable_total = nodes.iter().filter(|node| node.checklist_total > 0).count();
    let current_node_index = nodes
        .iter()
        .position(|node| node.checklist_total > 0 && !node.complete)
        .or_else(|| (executable_total == 0 && !nodes.is_empty()).then_some(0));
    let current_node_id = current_node_index.map(|index| nodes[index].id.clone());
    let current_node_title = current_node_index.map(|index| nodes[index].title.clone());
    AutonomyPlanStatus {
        total_nodes: if executable_total > 0 {
            executable_total
        } else {
            nodes.len()
        },
        completed_nodes: nodes
            .iter()
            .filter(|node| node.checklist_total > 0 && node.complete)
            .count(),
        current_node_id,
        current_node_title,
        current_node_index,
        evidence_dir: evidence_dir.map(|path| path.display().to_string()),
        current_evidence_path,
        current_evidence_valid,
        current_evidence_errors,
        nodes,
    }
}

pub(crate) fn read_autonomy_plan_status(
    cwd: &Path,
    plan_path: &Path,
    objective: &str,
) -> anyhow::Result<Option<AutonomyPlanStatus>> {
    let absolute_plan_path = cwd.join(plan_path);
    let markdown = match std::fs::read_to_string(&absolute_plan_path) {
        Ok(markdown) => markdown,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let paths = autonomy_library_paths_for_plan(plan_path);
    Ok(Some(autonomy_plan_status_with_evidence(
        &markdown,
        objective,
        Some(&paths.evidence_dir),
        cwd,
    )?))
}

pub(crate) fn current_autonomy_node_slices(
    cwd: &Path,
    plan_path: &Path,
    objective: &str,
) -> anyhow::Result<Option<(AutonomyPlanStatus, String, String)>> {
    let absolute_plan_path = cwd.join(plan_path);
    let markdown = match std::fs::read_to_string(&absolute_plan_path) {
        Ok(markdown) => markdown,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let paths = autonomy_library_paths_for_plan(plan_path);
    let status =
        autonomy_plan_status_with_evidence(&markdown, objective, Some(&paths.evidence_dir), cwd)?;
    let Some(node_id) = status.current_node_id.clone() else {
        return Ok(Some((
            status,
            "All compiled CLL nodes appear complete.".to_string(),
            "All compiled PLL prompt slices appear complete.".to_string(),
        )));
    };
    let plan = parse_autonomy_markdown_plan(&markdown, objective);
    let Some(node) = plan.nodes.iter().find(|node| node.id == node_id) else {
        return Ok(Some((
            status,
            format!("Current CLL node `{node_id}` was not found in the plan."),
            format!("Current PLL node `{node_id}` was not found in the plan."),
        )));
    };
    let cll = render_current_cll_slice(&plan, node, &status);
    let pll = render_current_pll_slice(node, &status);
    Ok(Some((status, cll, pll)))
}

pub(crate) fn evidence_packet_path(evidence_dir: &Path, node_id: &str) -> PathBuf {
    evidence_dir.join(format!("{node_id}.completion.json"))
}

pub(crate) fn validate_node_evidence(
    cwd: &Path,
    evidence_dir: &Path,
    node_id: &str,
    nodes: &[AutonomyNodeStatus],
) -> anyhow::Result<AutonomyEvidenceValidation> {
    let relative_path = evidence_packet_path(evidence_dir, node_id);
    let evidence_path = relative_path.display().to_string();
    let absolute_path = cwd.join(&relative_path);
    let Some(node) = nodes.iter().find(|node| node.id == node_id) else {
        return Ok(AutonomyEvidenceValidation {
            node_id: node_id.to_string(),
            evidence_path,
            valid: false,
            errors: vec!["current node not found in plan status".to_string()],
        });
    };
    let packet_text = match std::fs::read_to_string(&absolute_path) {
        Ok(packet_text) => packet_text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AutonomyEvidenceValidation {
                node_id: node_id.to_string(),
                evidence_path,
                valid: false,
                errors: vec!["completion evidence packet has not been written yet".to_string()],
            });
        }
        Err(error) => return Err(error.into()),
    };
    let packet = match serde_json::from_str::<AutonomyCompletionPacket>(&packet_text) {
        Ok(packet) => packet,
        Err(error) => {
            return Ok(AutonomyEvidenceValidation {
                node_id: node_id.to_string(),
                evidence_path,
                valid: false,
                errors: vec![format!(
                    "completion evidence packet is not valid JSON: {error}"
                )],
            });
        }
    };
    let mut errors = Vec::new();
    if packet.node_id != node_id {
        errors.push(format!(
            "packet node_id `{}` does not match current node `{node_id}`",
            packet.node_id
        ));
    }
    if !matches!(
        packet.status.as_str(),
        "complete" | "completed" | "blocked" | "partial"
    ) {
        errors.push(
            "packet status must be one of complete, completed, blocked, or partial".to_string(),
        );
    }
    if !matches!(packet.status.as_str(), "complete" | "completed") {
        errors.push("completion evidence packet status is not complete".to_string());
    }
    if matches!(packet.status.as_str(), "complete" | "completed") {
        if packet.actions_taken.is_empty() {
            errors.push("complete packet must include at least one action_taken".to_string());
        }
        if !node.expected_deliverables.is_empty() && packet.deliverables.is_empty() {
            errors.push("complete packet must include deliverables evidence".to_string());
        }
        if packet.success_conditions_satisfied.len() < node.success_conditions.len() {
            errors.push(format!(
                "complete packet maps {}/{} success conditions",
                packet.success_conditions_satisfied.len(),
                node.success_conditions.len()
            ));
        }
        if !node.validation.is_empty() && packet.verification.is_empty() {
            errors.push("complete packet must include verification evidence".to_string());
        }
    }
    Ok(AutonomyEvidenceValidation {
        node_id: node_id.to_string(),
        evidence_path,
        valid: errors.is_empty(),
        errors,
    })
}

fn write_current_evidence_template(
    cwd: &Path,
    evidence_dir: &Path,
    status: &AutonomyPlanStatus,
) -> anyhow::Result<()> {
    let Some(node_id) = status.current_node_id.as_deref() else {
        return Ok(());
    };
    let path = cwd.join(evidence_packet_path(evidence_dir, node_id));
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let template = AutonomyCompletionPacket {
        node_id: node_id.to_string(),
        status: "partial".to_string(),
        actions_taken: Vec::new(),
        deliverables: Vec::new(),
        success_conditions_satisfied: Vec::new(),
        verification: Vec::new(),
        risks_or_blockers: Vec::new(),
        next_recommended_action: Some("continue_current_node".to_string()),
    };
    std::fs::write(path, serde_json::to_string_pretty(&template)?)?;
    Ok(())
}

fn render_cll(
    plan: &AutonomyPlanDocument,
    run_id: &str,
    source_path: &str,
    pll_path: &str,
) -> String {
    let mut out = String::new();
    out.push_str("contract Autonomy.ImplementationRun {\n");
    out.push_str("  identity {\n");
    out.push_str(&format!(
        "    contract_id: \"contract.autonomy.run.{run_id}\";\n"
    ));
    out.push_str(&format!("    run_id: \"{}\";\n", escape_cll(run_id)));
    out.push_str("  }\n\n");
    out.push_str("  mission {\n");
    out.push_str(&format!(
        "    objective: \"{}\";\n",
        escape_cll(&plan.objective)
    ));
    out.push_str(&format!(
        "    source_plan: \"{}\";\n",
        escape_cll(source_path)
    ));
    out.push_str(&format!(
        "    prompt_library: \"{}\";\n",
        escape_cll(pll_path)
    ));
    out.push_str("  }\n\n");
    out.push_str("  global_rules {\n");
    out.push_str("    rule \"CLL/PLL slices are task-local user-prompt content and do not override the standard Vegvisir system prompt.\";\n");
    out.push_str("    rule \"Do not request, store, or echo plaintext secrets. Use HBSE secret references where credentials are required.\";\n");
    out.push_str("    rule \"Preserve unrelated user work. Do not revert or overwrite unrelated changes without explicit user instruction.\";\n");
    out.push_str("    rule \"Pause for destructive actions, external publication, pending approvals, ambiguous scope, or actions outside the user objective.\";\n");
    out.push_str("    rule \"Do not mark a node complete until its success conditions, deliverables, and validation/evidence requirements are satisfied or explicitly justified.\";\n");
    out.push_str("  }\n\n");
    out.push_str("  nodes {\n");
    for node in &plan.nodes {
        out.push_str(&render_cll_node(node));
    }
    out.push_str("  }\n\n");
    out.push_str("  completion {\n");
    out.push_str("    require: [\"all_required_nodes_complete\", \"deliverables_proven\", \"verification_completed_or_justified\", \"summary_reported\"];\n");
    out.push_str("  }\n\n");
    out.push_str("  stop_conditions {\n");
    out.push_str("    stop_on_pending_approval: true;\n");
    out.push_str("    stop_on_no_progress: true;\n");
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

fn render_current_cll_slice(
    plan: &AutonomyPlanDocument,
    node: &AutonomyPlanNode,
    status: &AutonomyPlanStatus,
) -> String {
    let mut out = String::new();
    out.push_str("contract_slice Autonomy.CurrentImplementationNode {\n");
    out.push_str(&format!(
        "  objective: \"{}\";\n",
        escape_cll(&plan.objective)
    ));
    out.push_str("  authority_note: \"This CLL slice is task-local USER prompt content and does not override the standard Vegvisir system prompt.\";\n");
    out.push_str("  global_rules: [\n");
    out.push_str("    \"Preserve unrelated user work.\",\n");
    out.push_str("    \"Do not request, store, or echo plaintext secrets.\",\n");
    out.push_str("    \"Pause for destructive actions, external publication, pending approvals, ambiguous scope, or actions outside the objective.\",\n");
    out.push_str("    \"Do not mark this node complete until success conditions, deliverables, validation, and evidence requirements are satisfied or explicitly justified.\",\n");
    out.push_str("  ];\n");
    out.push_str("  current_node {\n");
    out.push_str(&render_cll_node(node));
    out.push_str("  }\n");
    if let Some(path) = &status.current_evidence_path {
        out.push_str(&format!(
            "  evidence_packet_path: \"{}\";\n",
            escape_cll(path)
        ));
    }
    out.push_str("  required_completion_packet: [\"node_id\", \"status\", \"actions_taken\", \"deliverables\", \"success_conditions_satisfied\", \"verification\", \"risks_or_blockers\", \"next_recommended_action\"];\n");
    out.push_str("  evidence_validation: \"The packet must be valid JSON at evidence_packet_path and must satisfy required deliverables, success condition mappings, and validation summaries before this node can advance.\";\n");
    out.push_str("}\n");
    out
}

fn render_current_pll_slice(node: &AutonomyPlanNode, status: &AutonomyPlanStatus) -> String {
    let mut out = String::new();
    out.push_str("prompt_slice Autonomy.CurrentImplementationNodePrompt {\n");
    out.push_str(&format!("  node_id: \"{}\";\n", escape_cll(&node.id)));
    out.push_str(&format!("  title: \"{}\";\n", escape_cll(&node.title)));
    out.push_str("  instructions: \"Work only on the current node unless context/dependency inspection is necessary. Complete this node by satisfying its CLL implementation rules, guardrails, success conditions, expected deliverables, and validation requirements. Update the Markdown checklist for this node only after real completion. If blocked, report a structured blocker instead of guessing.\";\n");
    if let Some(path) = &status.current_evidence_path {
        out.push_str(&format!(
            "  evidence_packet_path: \"{}\";\n",
            escape_cll(path)
        ));
    }
    out.push_str("  required_response: \"Return a concise completion packet with node_id, status, actions_taken, deliverables, success_conditions_satisfied, verification, risks_or_blockers, and next_recommended_action. Also write/update the same packet as JSON at evidence_packet_path when claiming node completion.\";\n");
    out.push_str("}\n");
    out
}

fn render_cll_node(node: &AutonomyPlanNode) -> String {
    let mut out = String::new();
    let kind = node_kind(node.level);
    out.push_str(&format!("    {kind} \"{}\" {{\n", escape_cll(&node.id)));
    out.push_str(&format!("      title: \"{}\";\n", escape_cll(&node.title)));
    if let Some(parent) = &node.parent_id {
        out.push_str(&format!("      parent: \"{}\";\n", escape_cll(parent)));
    }
    out.push_str(&format!("      level: {};\n", node.level));
    out.push_str(&format!(
        "      status: \"{}\";\n",
        if node.checklist.iter().all(|item| item.checked) && !node.checklist.is_empty() {
            "complete"
        } else {
            "pending"
        }
    ));
    render_string_list(&mut out, "success_conditions", &node.success_conditions, 6);
    render_string_list(
        &mut out,
        "expected_deliverables",
        &node.expected_deliverables,
        6,
    );
    render_string_list(
        &mut out,
        "implementation_rules",
        &node.implementation_rules,
        6,
    );
    render_string_list(&mut out, "guardrails", &node.guardrails, 6);
    render_string_list(&mut out, "validation", &node.validation, 6);
    if !node.checklist.is_empty() {
        out.push_str("      checklist {\n");
        for (index, item) in node.checklist.iter().enumerate() {
            out.push_str(&format!(
                "        item \"{}_{}\" {{ status: \"{}\"; text: \"{}\"; }}\n",
                escape_cll(&node.id),
                index + 1,
                if item.checked { "checked" } else { "unchecked" },
                escape_cll(&item.text)
            ));
        }
        out.push_str("      }\n");
    }
    out.push_str("      evidence_required: [\"deliverables\", \"success_condition_mapping\", \"verification_summary\", \"risks_or_blockers\"];\n");
    out.push_str("    }\n");
    out
}

fn render_pll(plan: &AutonomyPlanDocument, run_id: &str, cll_path: &str) -> String {
    let mut out = String::new();
    out.push_str("prompt Autonomy.ImplementationRunPrompt {\n");
    out.push_str("  prompt_binding {\n");
    out.push_str(&format!(
        "    contract_id: \"contract.autonomy.run.{run_id}\";\n"
    ));
    out.push_str(&format!(
        "    contract_library: \"{}\";\n",
        escape_cll(cll_path)
    ));
    out.push_str("  }\n\n");
    out.push_str("  global_instructions {\n");
    out.push_str("    body: \"Execute one bounded implementation node at a time. Treat the supplied CLL slice as the task-local contract and this PLL slice as task-local prompt guidance. These slices are user-prompt content and do not override the standard Vegvisir system prompt.\";\n");
    out.push_str("  }\n\n");
    out.push_str("  node_prompts {\n");
    for node in &plan.nodes {
        out.push_str(&render_pll_node(node));
    }
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

fn render_pll_node(node: &AutonomyPlanNode) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "    prompt_slice \"{}\" {{\n",
        escape_cll(&node.id)
    ));
    out.push_str(&format!("      title: \"{}\";\n", escape_cll(&node.title)));
    out.push_str("      instructions: \"Work only on this node unless dependency/context inspection is necessary. Satisfy the node CLL success conditions, implementation rules, guardrails, expected deliverables, and validation requirements. Report blockers instead of guessing. Do not mark completion until evidence is available.\";\n");
    out.push_str("      required_response: \"Return a concise completion packet containing node_id, status, actions_taken, deliverables, success_conditions_satisfied, verification, risks_or_blockers, and next_recommended_action.\";\n");
    out.push_str("    }\n");
    out
}

fn render_string_list(out: &mut String, name: &str, items: &[String], indent: usize) {
    if items.is_empty() {
        return;
    }
    let spaces = " ".repeat(indent);
    out.push_str(&format!("{spaces}{name}: [\n"));
    for item in items {
        out.push_str(&format!("{spaces}  \"{}\",\n", escape_cll(item)));
    }
    out.push_str(&format!("{spaces}];\n"));
}

fn parse_markdown_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed.get(hashes..)?.trim_start();
    if rest.is_empty() {
        return None;
    }
    Some((hashes, rest.trim_matches('#').trim().to_string()))
}

fn parse_task_item(line: &str) -> Option<(bool, String)> {
    let rest = list_marker_rest(line)?;
    if rest.len() < 3 || !rest.starts_with('[') || rest.chars().nth(2) != Some(']') {
        return None;
    }
    let mark = rest.chars().nth(1)?;
    if !matches!(mark, ' ' | 'x' | 'X') {
        return None;
    }
    Some((matches!(mark, 'x' | 'X'), rest[3..].trim().to_string()))
}

fn parse_plain_list_item(line: &str) -> Option<String> {
    let rest = list_marker_rest(line)?;
    if rest.starts_with('[') {
        return None;
    }
    let trimmed = rest.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn list_marker_rest(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix('-')
        .or_else(|| trimmed.strip_prefix('*'))
        .or_else(|| trimmed.strip_prefix('+'))?;
    Some(rest.trim_start())
}

fn parse_semantic_label(line: &str) -> Option<SemanticList> {
    let normalized = line
        .trim()
        .trim_end_matches(':')
        .to_ascii_lowercase()
        .replace(['-', '_'], " ");
    match normalized.as_str() {
        "success conditions" | "success criteria" | "completion conditions" => {
            Some(SemanticList::SuccessConditions)
        }
        "expected deliverables" | "deliverables" => Some(SemanticList::ExpectedDeliverables),
        "implementation rules" | "rules" | "guidelines" | "implementation guidelines" => {
            Some(SemanticList::ImplementationRules)
        }
        "guardrails" | "constraints" | "safety" => Some(SemanticList::Guardrails),
        "validation" | "verification" | "tests" => Some(SemanticList::Validation),
        _ => None,
    }
}

fn node_id_from_title(ordinal_path: &[usize], title: &str) -> String {
    let prefix = match ordinal_path.len() {
        0 => "node".to_string(),
        1 => format!("phase_{:02}", ordinal_path[0]),
        2 => format!(
            "phase_{:02}_section_{:02}",
            ordinal_path[0], ordinal_path[1]
        ),
        3 => format!(
            "phase_{:02}_section_{:02}_subsection_{:02}",
            ordinal_path[0], ordinal_path[1], ordinal_path[2]
        ),
        _ => {
            let mut id = format!(
                "phase_{:02}_section_{:02}_subsection_{:02}",
                ordinal_path[0], ordinal_path[1], ordinal_path[2]
            );
            for extra in ordinal_path.iter().skip(3) {
                id.push_str(&format!("_part_{extra:02}"));
            }
            id
        }
    };
    let slug = slugify(title);
    if slug.is_empty() {
        prefix
    } else {
        format!("{prefix}_{slug}")
    }
}

fn node_kind(level: usize) -> &'static str {
    match level {
        1 | 2 => "phase",
        3 => "section",
        _ => "subsection",
    }
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep && !out.is_empty() {
            out.push('_');
            last_was_sep = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

fn escape_cll(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_into_stable_hierarchy_and_semantic_lists() {
        let markdown = r#"# Implementation Plan
## Phase 1: Foundation
Success conditions:
- Foundation success
Expected deliverables:
- Foundation artifact
Implementation rules:
- Keep it deterministic
Guardrails:
- Preserve user work
Validation:
- cargo test autonomy_plan
- [ ] Inspect code
### Section 1.1: Parser
- [x] Define AST
#### Subsection 1.1.1: Markdown AST
- [ ] Parse headings
"#;
        let plan = parse_autonomy_markdown_plan(markdown, "Build autonomy compiler");
        assert_eq!(plan.nodes.len(), 4);
        assert_eq!(plan.nodes[1].id, "phase_01_section_01_phase_1_foundation");
        assert_eq!(
            plan.nodes[2].parent_id.as_deref(),
            Some("phase_01_section_01_phase_1_foundation")
        );
        assert_eq!(plan.nodes[1].success_conditions, vec!["Foundation success"]);
        assert_eq!(
            plan.nodes[1].expected_deliverables,
            vec!["Foundation artifact"]
        );
        assert_eq!(
            plan.nodes[1].implementation_rules,
            vec!["Keep it deterministic"]
        );
        assert_eq!(plan.nodes[1].guardrails, vec!["Preserve user work"]);
        assert_eq!(plan.nodes[1].validation, vec!["cargo test autonomy_plan"]);
        assert_eq!(plan.nodes[2].checklist[0].checked, true);
    }

    #[test]
    fn compiles_markdown_to_cll_pll_and_manifest() -> anyhow::Result<()> {
        let markdown = r#"# Plan
## Phase 1: Compiler
Success conditions:
- CLL generated
Expected deliverables:
- implementation.cll
- [ ] Implement compiler
"#;
        let compiled = compile_autonomy_plan_libraries(
            markdown,
            "Compile autonomy libraries",
            "run-1",
            ".vegvisir/autonomy/run-plan.md",
            ".vegvisir/autonomy/run-plan.cll",
            ".vegvisir/autonomy/run-plan.pll",
        )?;
        assert!(compiled.cll.contains("contract Autonomy.ImplementationRun"));
        assert!(compiled.cll.contains("success_conditions"));
        assert!(compiled.cll.contains("expected_deliverables"));
        assert!(compiled.pll.contains("prompt_slice"));
        assert!(compiled.manifest.contains(AUTONOMY_COMPILER_VERSION));
        Ok(())
    }

    #[test]
    fn validates_current_node_completion_evidence() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path();
        std::fs::create_dir_all(workspace.join(".vegvisir/autonomy"))?;
        let plan_path = Path::new(".vegvisir/autonomy/run-plan.md");
        std::fs::write(
            workspace.join(plan_path),
            "# Plan
## Phase 1: Evidence
Success conditions:
- Condition mapped
Expected deliverables:
- Artifact produced
Validation:
- cargo test autonomy
- [ ] produce evidence
",
        )?;
        let paths = write_autonomy_libraries(workspace, plan_path, "objective", "run")?;
        let status = read_autonomy_plan_status(workspace, plan_path, "objective")?.unwrap();
        assert!(!status.current_evidence_valid);
        let packet_path = workspace.join(evidence_packet_path(
            &paths.evidence_dir,
            status.current_node_id.as_deref().unwrap(),
        ));
        std::fs::write(
            &packet_path,
            serde_json::json!({
                "node_id": status.current_node_id.as_deref().unwrap(),
                "status": "complete",
                "actions_taken": ["implemented evidence scaffold"],
                "deliverables": [{"type": "file", "path": "vegvisir/src/app/commands/autonomy_plan.rs", "description": "evidence validation"}],
                "success_conditions_satisfied": [{"condition": "Condition mapped", "evidence": "test packet maps it"}],
                "verification": [{"command": "cargo test autonomy", "result": "passed", "summary": "focused tests passed"}],
                "risks_or_blockers": [],
                "next_recommended_action": "advance"
            })
            .to_string(),
        )?;
        let status = read_autonomy_plan_status(workspace, plan_path, "objective")?.unwrap();
        assert!(
            status.current_evidence_valid,
            "{:?}",
            status.current_evidence_errors
        );
        Ok(())
    }

    #[test]
    fn status_selects_first_incomplete_node_and_current_slices() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path();
        std::fs::create_dir_all(workspace.join(".vegvisir/autonomy"))?;
        let plan_path = Path::new(".vegvisir/autonomy/run-plan.md");
        std::fs::write(
            workspace.join(plan_path),
            "# Plan\n## Phase 1: Done\n- [x] completed\n## Phase 2: Current\nSuccess conditions:\n- Current success\nExpected deliverables:\n- Current artifact\n- [ ] implement current\n",
        )?;
        let paths = write_autonomy_libraries(workspace, plan_path, "objective", "run")?;
        let first_status = read_autonomy_plan_status(workspace, plan_path, "objective")?.unwrap();
        assert_eq!(first_status.completed_nodes, 0);
        assert_eq!(
            first_status.current_node_title.as_deref(),
            Some("Phase 1: Done")
        );
        let done_packet_path = workspace.join(evidence_packet_path(
            &paths.evidence_dir,
            first_status.current_node_id.as_deref().unwrap(),
        ));
        std::fs::write(
            done_packet_path,
            serde_json::json!({
                "node_id": first_status.current_node_id.as_deref().unwrap(),
                "status": "complete",
                "actions_taken": ["completed first node"],
                "deliverables": [],
                "success_conditions_satisfied": [],
                "verification": [],
                "risks_or_blockers": [],
                "next_recommended_action": "advance"
            })
            .to_string(),
        )?;
        let Some((status, cll, pll)) =
            current_autonomy_node_slices(workspace, plan_path, "objective")?
        else {
            panic!("expected current slices");
        };
        assert_eq!(status.completed_nodes, 1);
        assert_eq!(status.total_nodes, 2);
        assert_eq!(
            status.current_node_title.as_deref(),
            Some("Phase 2: Current")
        );
        assert!(cll.contains("Current success"));
        assert!(cll.contains("Current artifact"));
        assert!(pll.contains("Phase 2: Current"));
        assert!(
            workspace
                .join(".vegvisir/autonomy/run-plan-state.json")
                .exists()
        );
        Ok(())
    }
}
