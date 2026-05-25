use std::path::Path;

use crate::{
    core::SessionState,
    lsl::{LslSkillTrace, load_or_compile_lsl_roots},
};

use super::LslRuntimeConfig;

pub(crate) fn prepare_lsl_augmented_content(
    cwd: &Path,
    data_root: &Path,
    content: &str,
    session: &SessionState,
    config: &LslRuntimeConfig,
) -> anyhow::Result<(String, Option<LslSkillTrace>)> {
    if matches!(config.mode.as_str(), "off" | "manual" | "manual_only") {
        return Ok((content.to_string(), None));
    }
    let roots = vec![
        cwd.join(".vegvisir").join("skills"),
        cwd.join("skills"),
        data_root.join("skills"),
    ];
    let compiled_root = cwd.join(".vegvisir").join("compiled");
    let compiled = match load_or_compile_lsl_roots(&roots, &compiled_root) {
        Ok(compiled) => compiled,
        Err(_) => return Ok((content.to_string(), None)),
    };
    if !compiled.registry.issues.is_empty() {
        return Ok((content.to_string(), None));
    }
    let selected = compiled
        .registry
        .route(content, config.max_primary_subskills)
        .into_iter()
        .take(config.max_primary_subskills)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Ok((
            content.to_string(),
            Some(LslSkillTrace {
                event: "auto_no_match".to_string(),
                query: redact_trace_query(content),
                selected: Vec::new(),
                token_estimate: 0,
                available_tokens: config.token_budget,
                remaining_tokens: config.token_budget,
                provider: Some(session.current_provider.clone()),
                model: Some(session.current_model.clone()),
                created_at: chrono::Utc::now().to_rfc3339(),
                ..LslSkillTrace::default()
            }),
        ));
    }
    let context = compiled.registry.load_context_for_query(
        &selected,
        content,
        config.token_budget,
        config.max_dependency_depth,
    );
    if context.selected.is_empty() || config.mode == "suggestions" {
        let trace_selected = context
            .selected
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        return Ok((
            content.to_string(),
            Some(LslSkillTrace {
                event: if trace_selected.is_empty() {
                    "auto_blocked"
                } else {
                    "auto_suggest"
                }
                .to_string(),
                query: redact_trace_query(content),
                selected: trace_selected,
                token_estimate: context.used_tokens,
                available_tokens: context.available_tokens,
                remaining_tokens: context.remaining_tokens,
                load_modes: context
                    .selected
                    .iter()
                    .map(|item| (item.id.clone(), item.mode.clone()))
                    .collect(),
                policy_decisions: context.policy_decisions.clone(),
                blocked: context.blocked.clone(),
                excluded: context.excluded.clone(),
                not_loaded_relevant: context.not_loaded_relevant.clone(),
                provider: Some(session.current_provider.clone()),
                model: Some(session.current_model.clone()),
                created_at: chrono::Utc::now().to_rfc3339(),
                ..LslSkillTrace::default()
            }),
        ));
    }
    let already_enabled = session
        .active_agent_id
        .is_some()
        .then(|| {
            session
                .enabled_skills
                .iter()
                .map(|skill| skill.name.clone())
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let skill_sections = context
        .selected
        .iter()
        .filter(|loaded| !already_enabled.contains(&loaded.id))
        .map(|loaded| {
            format!(
                "== {} [{}; {}]\n{}",
                loaded.id, loaded.mode, loaded.reason, loaded.text
            )
        })
        .collect::<Vec<_>>();
    if skill_sections.is_empty() {
        return Ok((content.to_string(), None));
    }
    let augmented = format!(
        "{content}\n\nLinked Skill Library context (auto-selected; tokens {}/{}):\n{}",
        context.used_tokens,
        context.available_tokens,
        skill_sections.join("\n\n")
    );
    Ok((
        augmented,
        Some(LslSkillTrace {
            event: "auto_load".to_string(),
            query: redact_trace_query(content),
            selected: context
                .selected
                .iter()
                .map(|item| item.id.clone())
                .collect(),
            token_estimate: context.used_tokens,
            available_tokens: context.available_tokens,
            remaining_tokens: context.remaining_tokens,
            load_modes: context
                .selected
                .iter()
                .map(|item| (item.id.clone(), item.mode.clone()))
                .collect(),
            policy_decisions: context.policy_decisions.clone(),
            blocked: context.blocked.clone(),
            excluded: context.excluded.clone(),
            not_loaded_relevant: context.not_loaded_relevant.clone(),
            provider: Some(session.current_provider.clone()),
            model: Some(session.current_model.clone()),
            created_at: chrono::Utc::now().to_rfc3339(),
            ..LslSkillTrace::default()
        }),
    ))
}

pub(crate) fn redact_trace_query(content: &str) -> String {
    let compact = content
        .split_whitespace()
        .take(24)
        .collect::<Vec<_>>()
        .join(" ");
    if compact.len() > 240 {
        format!("{}…", &compact[..240])
    } else {
        compact
    }
}

pub(crate) fn compiled_lsl_selected_from_trace(
    cwd: &Path,
    data_root: &Path,
    content: &str,
    config: &LslRuntimeConfig,
) -> Vec<crate::lsl::LoadedSubskill> {
    let roots = vec![
        cwd.join(".vegvisir").join("skills"),
        cwd.join("skills"),
        data_root.join("skills"),
    ];
    let compiled_root = cwd.join(".vegvisir").join("compiled");
    let Ok(compiled) = load_or_compile_lsl_roots(&roots, &compiled_root) else {
        return Vec::new();
    };
    let selected = compiled
        .registry
        .route(content, config.max_primary_subskills);
    compiled
        .registry
        .load_context_for_query(
            &selected,
            content,
            config.token_budget,
            config.max_dependency_depth,
        )
        .selected
}
