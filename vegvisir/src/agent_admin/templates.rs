use anyhow::Context;
use serde_json::{Value, json};

use crate::core::{AgentProfile, normalize_agent_id};

use super::models::AgentTemplate;

pub fn profile_from_template(
    mode: &str,
    id: &str,
    name_override: Option<&str>,
) -> anyhow::Result<AgentProfile> {
    let template = agent_template(mode).with_context(|| format!("unknown template: {mode}"))?;
    let mut profile = AgentProfile::new(
        id,
        name_override.unwrap_or(&template.display_name),
        &template.system_prompt,
    )?;
    profile.mode = template.mode.clone();
    profile.description = template.description.clone();
    profile.enabled_tools = template.enabled_tools.clone();
    profile.enabled_skills = template.enabled_skills.clone();
    profile.usrl_contracts = template.usrl_contracts.clone();
    profile.memory_policy = template.memory_policy.clone();
    profile
        .metadata
        .insert("template".to_string(), Value::String(template.mode));
    profile
        .metadata
        .insert("registered_identity".to_string(), Value::Bool(false));
    profile
        .metadata
        .insert("identity_source".to_string(), json!("agent-admin-template"));
    Ok(profile)
}

pub fn agent_template(mode: &str) -> Option<AgentTemplate> {
    let normalized = normalize_agent_id(mode);
    agent_templates()
        .into_iter()
        .find(|template| template.mode == normalized)
}

pub fn agent_templates() -> Vec<AgentTemplate> {
    vec![
        template(
            "planner",
            "Planner",
            "Decomposes goals into staged, verifiable plans.",
            "You are a planning specialist. Convert ambiguous goals into concrete phases, dependencies, risks, acceptance checks, and next actions. Do not edit files unless explicitly asked through an enabled tool path.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_prepare_context",
                "save_session",
            ],
        ),
        template(
            "researcher",
            "Researcher",
            "Finds, compares, and summarizes project evidence.",
            "You are a research specialist. Gather relevant local context, distinguish evidence from inference, cite files or memories when available, and produce concise findings with uncertainty called out.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_prepare_context",
                "cms_remember",
            ],
        ),
        template(
            "orchestrator",
            "Orchestrator",
            "Coordinates specialist agents and tracks execution state.",
            "You are an orchestration specialist. Break work into bounded tasks, delegate when useful, merge results, maintain task state, and keep execution aligned with the user's current objective.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_prepare_context",
                "spawn_subagent",
                "save_session",
                "audit_log",
            ],
        ),
        template(
            "engineer",
            "Engineer",
            "Implements scoped code changes with verification.",
            "You are an engineering specialist. Read the surrounding code before changing it, make minimal coherent edits, preserve existing behavior unless intentionally changed, and verify with focused tests.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "cms_prepare_context",
                "audit_log",
            ],
        ),
        template(
            "coder",
            "Coder",
            "Focuses on implementation details and local patches.",
            "You are a coding specialist. Implement the requested behavior directly, keep patches small, follow local style, and report the exact verification performed.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
            ],
        ),
        template(
            "tester",
            "Tester",
            "Designs and runs verification for changed behavior.",
            "You are a testing specialist. Identify behavioral risk, add or run targeted tests, explain failures in terms of expected versus actual behavior, and avoid unrelated rewrites.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "audit_log",
            ],
        ),
        template_with_skills(
            "agent-red",
            "Agent Red",
            "Security-oriented review and adversarial analysis with delegated reconnaissance, risk gating, and evidence-backed mitigation planning.",
            "You are Agent Red, a security specialist for authorized defensive review. Focus on abuse cases, privilege boundaries, secret handling, prompt/tool injection paths, unsafe execution, supply-chain risk, and concrete mitigations. Work evidence-first: inspect relevant files, tests, traces, memories, and tool outputs before making security claims. Use bounded subagents for independent reconnaissance or test planning when the review is broad. Use CMS context tools only for non-secret project memory and never request, expose, transform, or store plaintext secrets. Treat offensive techniques as analysis context only; do not provide persistence, stealth, credential theft, exploitation deployment, or unauthorized access guidance.",
            &[
                "list_files",
                "read_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "cms_prepare_context",
                "cms_prepare_model_request",
                "spawn_subagent",
                "audit_log",
            ],
            &[
                "repo-orientation",
                "code-review",
                "test-repair",
                "risk-check",
            ],
        ),
    ]
}

fn template(
    mode: &str,
    display_name: &str,
    description: &str,
    system_prompt: &str,
    enabled_tools: &[&str],
) -> AgentTemplate {
    template_with_skills(
        mode,
        display_name,
        description,
        system_prompt,
        enabled_tools,
        &[],
    )
}

fn template_with_skills(
    mode: &str,
    display_name: &str,
    description: &str,
    system_prompt: &str,
    enabled_tools: &[&str],
    enabled_skills: &[&str],
) -> AgentTemplate {
    AgentTemplate {
        mode: mode.to_string(),
        display_name: display_name.to_string(),
        description: description.to_string(),
        system_prompt: system_prompt.to_string(),
        enabled_tools: enabled_tools.iter().map(|tool| tool.to_string()).collect(),
        enabled_skills: enabled_skills
            .iter()
            .map(|skill| skill.to_string())
            .collect(),
        usrl_contracts: Vec::new(),
        memory_policy: "agent-scoped".to_string(),
    }
}
