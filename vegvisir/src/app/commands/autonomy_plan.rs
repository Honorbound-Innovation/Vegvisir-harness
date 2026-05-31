use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
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
    Ok(paths)
}

pub(crate) fn read_current_autonomy_slices(
    cwd: &Path,
    plan_path: &Path,
) -> anyhow::Result<Option<(String, String)>> {
    let paths = autonomy_library_paths_for_plan(plan_path);
    let cll = match std::fs::read_to_string(cwd.join(paths.cll_path)) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let pll = match std::fs::read_to_string(cwd.join(paths.pll_path)) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some((cll, pll)))
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
}
