use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    time::Duration,
};

use super::cli::{
    BudgetArgs, CreateArgs, CreateTemplateArgs, DesignArgs, ListArgs, PromptArgs, RegisterArgs,
    ScopeArgs, SetArgs,
};
use super::*;
use crate::core::{
    AgentProfile, AgentProfileStore, ModelRegistry, ProviderRegistry, load_skill_definitions,
    normalize_agent_id,
};
use anyhow::{Context, bail};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use serde_json::{Value, json};

pub(super) struct AgentRegistryAdmin {
    pub(super) data_root: PathBuf,
    pub(super) workspace: PathBuf,
    pub(super) store: AgentProfileStore,
}

#[derive(Copy, Clone)]
pub(super) enum ListField {
    Tools,
    Skills,
    Mcp,
    Usrl,
}

impl ListField {
    fn label(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Skills => "skills",
            Self::Mcp => "mcp_servers",
            Self::Usrl => "usrl_contracts",
        }
    }

    fn get_mut<'a>(self, profile: &'a mut AgentProfile) -> &'a mut Vec<String> {
        match self {
            Self::Tools => &mut profile.enabled_tools,
            Self::Skills => &mut profile.enabled_skills,
            Self::Mcp => &mut profile.enabled_mcp_servers,
            Self::Usrl => &mut profile.usrl_contracts,
        }
    }
}

impl AgentRegistryAdmin {
    pub(super) fn new(data_root: PathBuf, workspace: PathBuf) -> anyhow::Result<Self> {
        let store = AgentProfileStore::new(data_root.join("agents"))?;
        Ok(Self {
            data_root,
            workspace,
            store,
        })
    }

    pub(super) fn append_history(
        &self,
        profile: &AgentProfile,
        action: &str,
        path: &Path,
    ) -> anyhow::Result<()> {
        super::history::append_history(&self.data_root, profile, action, path)
    }

    pub(super) fn load_history(&self) -> anyhow::Result<Vec<HistoryEvent>> {
        super::history::load_history(&self.data_root)
    }

    pub(super) fn validate_profile(&self, profile: &AgentProfile) -> anyhow::Result<ValidationReport> {
        super::validation::validate_profile(profile, &self.workspace, &self.data_root)
    }

    pub(super) fn ensure_can_write(&self, id: &str, force: bool) -> anyhow::Result<()> {
        let normalized = normalize_agent_id(id);
        if normalized.is_empty() {
            bail!("agent id must contain at least one letter or number");
        }
        let path = self.store.path_for(&normalized);
        if path.exists() && !force {
            bail!(
                "profile {normalized} already exists at {}; use --force to overwrite",
                path.display()
            );
        }
        Ok(())
    }

    pub(super) fn save_touched(
        &self,
        mut profile: AgentProfile,
        action: &str,
        json_output: bool,
    ) -> anyhow::Result<()> {
        touch_metadata(&mut profile, action);
        profile.updated_at = chrono::Utc::now();
        let path = self.store.save(&profile)?;
        self.append_history(&profile, action, &path)?;
        print_saved(&profile, &path, json_output)
    }

    pub(super) fn save_touched_quiet(&self, mut profile: AgentProfile, action: &str) -> anyhow::Result<()> {
        touch_metadata(&mut profile, action);
        profile.updated_at = chrono::Utc::now();
        let path = self.store.save(&profile)?;
        self.append_history(&profile, action, &path)?;
        Ok(())
    }

    pub(super) fn register_skiller_pack(
        &self,
        known_ids: &mut BTreeSet<String>,
        path: &Path,
        dry_run: bool,
    ) -> anyhow::Result<Option<String>> {
        let pack: SkillerAgentPackOnDisk = serde_yaml::from_str(&fs::read_to_string(path)?)?;
        let id = normalize_agent_id(&pack.agent_name);
        if id.is_empty() || known_ids.contains(&id) {
            return Ok(None);
        }
        known_ids.insert(id.clone());
        if dry_run {
            return Ok(Some(id));
        }
        let prompt = if pack.system_prompt_material.trim().is_empty() {
            format!(
                "You are {}. Operate as a Skiller-generated specialist agent for source-grounded technical work. Use selected Skiller skills as evidence, respect runtime policy, cite sources when possible, and escalate unsupported or unsafe requests.",
                pack.agent_name
            )
        } else {
            pack.system_prompt_material.clone()
        };
        let mut profile = AgentProfile::new(&id, &pack.agent_name, prompt)?;
        profile.mode = "skiller".to_string();
        profile.description = if pack.description.trim().is_empty() {
            format!("Skiller-generated agent pack loaded from {}", path.display())
        } else {
            pack.description.clone()
        };
        profile.enabled_skills = pack.skill_ids.clone();
        profile.enabled_tools = pack
            .tool_permissions
            .iter()
            .filter_map(|permission| permission.split(':').next())
            .map(str::trim)
            .filter(|tool| !tool.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        profile.memory_policy = if pack.memory_policy.trim().is_empty() {
            "agent-scoped".to_string()
        } else {
            pack.memory_policy.clone()
        };
        profile.metadata.insert("registered_identity".to_string(), json!(true));
        profile.metadata.insert("identity_source".to_string(), json!("skiller-agent-pack"));
        profile
            .metadata
            .insert("artifact_path".to_string(), json!(path.display().to_string()));
        profile
            .metadata
            .insert("source_bundle_ids".to_string(), json!(pack.source_bundle_ids));
        profile
            .metadata
            .insert("source_bundle_name".to_string(), json!(pack.source_bundle_name));
        profile
            .metadata
            .insert("source_bundle_version".to_string(), json!(pack.source_bundle_version));
        let saved = self.store.save(&profile)?;
        self.append_history(&profile, "register-skiller-pack", &saved)?;
        Ok(Some(id))
    }

    pub(super) fn register_skiller_proposals(
        &self,
        known_ids: &mut BTreeSet<String>,
        index_path: &Path,
        dry_run: bool,
    ) -> anyhow::Result<Vec<String>> {
        let index: skiller::agents::AgentProposalIndex =
            serde_yaml::from_str(&fs::read_to_string(index_path)?)?;
        let base = index_path.parent().unwrap_or_else(|| Path::new("."));
        let mut created = Vec::new();
        for entry in &index.proposals {
            if entry.file.contains("..") || Path::new(&entry.file).is_absolute() {
                continue;
            }
            let proposal_path = base.join(&entry.file);
            let proposal: skiller::models::AgentProfileProposal =
                serde_yaml::from_str(&fs::read_to_string(&proposal_path)?)?;
            if let Some(id) = self.register_skiller_proposal(
                known_ids,
                &index,
                &proposal,
                &proposal_path,
                dry_run,
            )? {
                created.push(id);
            }
        }
        Ok(created)
    }

    fn register_skiller_proposal(
        &self,
        known_ids: &mut BTreeSet<String>,
        index: &skiller::agents::AgentProposalIndex,
        proposal: &skiller::models::AgentProfileProposal,
        path: &Path,
        dry_run: bool,
    ) -> anyhow::Result<Option<String>> {
        let id = normalize_agent_id(&proposal.agent_id);
        if id.is_empty() || known_ids.contains(&id) {
            return Ok(None);
        }
        known_ids.insert(id.clone());
        if dry_run {
            return Ok(Some(id));
        }
        let prompt = format!(
            "You are {}.\n\nPurpose: {}\n\nRuntime context policy: {}\nReview policy: {}\nEscalation policy: {}\n\nRecommended Skiller skills:\n{}",
            proposal.agent_name,
            proposal.agent_purpose,
            proposal.runtime_context_policy,
            proposal.review_policy,
            proposal.escalation_policy,
            proposal
                .recommended_skills
                .iter()
                .map(|skill| format!("- {skill}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let mut profile = AgentProfile::new(&id, &proposal.agent_name, prompt)?;
        profile.mode = "skiller".to_string();
        profile.description = proposal.agent_purpose.clone();
        profile.enabled_skills = proposal.recommended_skills.clone();
        profile.enabled_tools = proposal.required_tools.clone();
        profile.memory_policy = "agent-scoped".to_string();
        profile
            .metadata
            .insert("registered_identity".to_string(), json!(true));
        profile
            .metadata
            .insert("identity_source".to_string(), json!("skiller-agent-proposal"));
        profile
            .metadata
            .insert("artifact_path".to_string(), json!(path.display().to_string()));
        profile
            .metadata
            .insert("source_bundle_id".to_string(), json!(index.bundle_id));
        profile
            .metadata
            .insert("source_bundle_name".to_string(), json!(index.bundle_name));
        profile.metadata.insert(
            "ready_for_packaging".to_string(),
            json!(proposal.proposal_readiness.ready_for_packaging),
        );
        profile.metadata.insert(
            "ready_for_default_use_candidate".to_string(),
            json!(proposal.proposal_readiness.ready_for_default_use_candidate),
        );
        let saved = self.store.save(&profile)?;
        self.append_history(&profile, "register-skiller-proposal", &saved)?;
        Ok(Some(id))
    }

pub(super) fn print_paths(&self, json_output: bool) -> anyhow::Result<()> {
    print_json_or_text(
        json_output,
        &json!({
            "data_root": self.data_root,
            "agents_root": self.store.root,
            "workspace": self.workspace,
        }),
        || {
            println!("data_root: {}", self.data_root.display());
            println!("agents_root: {}", self.store.root.display());
            println!("workspace: {}", self.workspace.display());
            Ok(())
        },
    )
}

pub(super) fn doctor(&self, json_output: bool) -> anyhow::Result<()> {
    let (profiles, invalid_files) = self.store.list_lossy()?;
    let mut warnings = Vec::new();
    let mut ids = BTreeSet::new();
    let mut cms_scopes: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for profile in &profiles {
        if !ids.insert(profile.id.clone()) {
            warnings.push(format!("duplicate profile id loaded: {}", profile.id));
        }
        let expected_path = self.store.path_for(&profile.id);
        if !expected_path.exists() {
            warnings.push(format!(
                "profile {} is not stored at expected path {}",
                profile.id,
                expected_path.display()
            ));
        }
        if profile.system_prompt.trim().is_empty() {
            warnings.push(format!("profile {} has an empty system prompt", profile.id));
        }
        if profile.display_name.trim().is_empty() {
            warnings.push(format!("profile {} has an empty display name", profile.id));
        }
        if profile
            .enabled_tools
            .iter()
            .any(|tool| tool.trim().is_empty())
        {
            warnings.push(format!("profile {} has an empty tool entry", profile.id));
        }
        if profile.current_model.is_some() && profile.current_provider.is_none() {
            warnings.push(format!(
                "profile {} sets a model but no provider; runtime will inherit current provider",
                profile.id
            ));
        }
        cms_scopes
            .entry((profile.cms_user_id.clone(), profile.cms_project_id.clone()))
            .or_default()
            .push(profile.id.clone());
    }
    for ((user, project), profile_ids) in cms_scopes {
        if profile_ids.len() > 1 {
            warnings.push(format!(
                "profiles share CMS scope {user}/{project}: {}",
                profile_ids.join(",")
            ));
        }
    }
    let report = DoctorReport {
        agents_root: self.store.root.clone(),
        profile_count: profiles.len(),
        invalid_files,
        warnings,
    };
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Agent registry: {}", report.agents_root.display());
        println!("Profiles: {}", report.profile_count);
        if report.invalid_files.is_empty() && report.warnings.is_empty() {
            println!("Status: ok");
        }
        if !report.invalid_files.is_empty() {
            println!("\nInvalid files:");
            for item in &report.invalid_files {
                println!("- {item}");
            }
        }
        if !report.warnings.is_empty() {
            println!("\nWarnings:");
            for item in &report.warnings {
                println!("- {item}");
            }
        }
    }
    Ok(())
}

pub(super) fn register(&self, args: RegisterArgs, json_output: bool) -> anyhow::Result<()> {
    let mut report = RegisterReport {
        dry_run: args.dry_run,
        ..RegisterReport::default()
    };
    let (profiles, warnings) = self.store.list_lossy()?;
    report.warnings.extend(warnings);
    let mut known_ids = profiles
        .into_iter()
        .map(|profile| profile.id)
        .collect::<BTreeSet<_>>();

    if !args.skiller_only {
        for template in agent_templates() {
            if known_ids.contains(&template.mode) {
                continue;
            }
            report.created_ids.push(template.mode.clone());
            report.builtin_created += 1;
            if !args.dry_run {
                let mut profile = profile_from_template(&template.mode, &template.mode, None)?;
                profile
                    .metadata
                    .insert("registered_identity".to_string(), Value::Bool(true));
                profile
                    .metadata
                    .insert("identity_source".to_string(), json!("builtin-template"));
                touch_metadata(&mut profile, "register-builtin");
                let saved = self.store.save(&profile)?;
                self.append_history(&profile, "register-builtin", &saved)?;
            }
            known_ids.insert(template.mode);
        }
    }

    if !args.builtins_only {
        for artifact in find_skiller_agent_artifacts(&self.workspace, &self.data_root) {
            match artifact {
                Ok(SkillerAgentArtifact::Pack(path)) => {
                    match self.register_skiller_pack(&mut known_ids, &path, args.dry_run) {
                        Ok(Some(id)) => {
                            report.skiller_created += 1;
                            report.created_ids.push(id);
                        }
                        Ok(None) => {}
                        Err(error) => report.warnings.push(format!(
                            "skipped Skiller agent pack {}: {error}",
                            path.display()
                        )),
                    }
                }
                Ok(SkillerAgentArtifact::ProposalIndex(path)) => {
                    match self.register_skiller_proposals(&mut known_ids, &path, args.dry_run) {
                        Ok(ids) => {
                            report.skiller_created += ids.len();
                            report.created_ids.extend(ids);
                        }
                        Err(error) => report.warnings.push(format!(
                            "skipped Skiller agent proposal index {}: {error}",
                            path.display()
                        )),
                    }
                }
                Err(error) => report.warnings.push(error.to_string()),
            }
        }
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Registered built-in agents: {}", report.builtin_created);
        println!("Registered Skiller agents: {}", report.skiller_created);
        if args.dry_run {
            println!("Dry run: no profiles written.");
        }
        if !report.created_ids.is_empty() {
            println!("IDs: {}", report.created_ids.join(","));
        }
        if !report.warnings.is_empty() {
            println!("\nWarnings:");
            for warning in &report.warnings {
                println!("- {warning}");
            }
        }
    }
    Ok(())
}

pub(super) fn validate(&self, id: Option<&str>, json_output: bool) -> anyhow::Result<()> {
    if let Some(id) = id {
        let profile = self.store.load(id)?;
        let report = self.validate_profile(&profile)?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_validation_report(&report);
        }
        return Ok(());
    }
    let (profiles, warnings) = self.store.list_lossy()?;
    let mut reports = Vec::new();
    for profile in &profiles {
        reports.push(self.validate_profile(profile)?);
    }
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "agents_root": self.store.root,
                "load_warnings": warnings,
                "reports": reports,
            }))?
        );
    } else {
        println!(
            "Validated {} profile(s) in {}",
            reports.len(),
            self.store.root.display()
        );
        for warning in warnings {
            println!("Load warning: {warning}");
        }
        for report in &reports {
            print_validation_report(report);
        }
    }
    Ok(())
}

pub(super) fn metrics(&self, id: &str, json_output: bool) -> anyhow::Result<()> {
    self.store.load(id)?;
    let mut report = load_metrics_report(&self.data_root, id)?;
    report.id = normalize_agent_id(id);
    let task_total = report.metrics.tasks_completed
        + report.metrics.tasks_failed
        + report.metrics.tasks_cancelled;
    report.task_success_rate = ratio(report.metrics.tasks_completed, task_total);
    if task_total == 0 {
        report
            .warnings
            .push("no recorded task metrics for this agent".to_string());
    }
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_metrics_report(&report);
    }
    Ok(())
}

pub(super) fn compare(
    &self,
    left_id: &str,
    right_id: &str,
    prompts: bool,
    json_output: bool,
) -> anyhow::Result<()> {
    let left = self.store.load(left_id)?;
    let right = self.store.load(right_id)?;
    let mut differences = Vec::new();
    push_diff(
        &mut differences,
        "mode",
        json!(left.mode),
        json!(right.mode),
    );
    push_diff(
        &mut differences,
        "display_name",
        json!(left.display_name),
        json!(right.display_name),
    );
    push_diff(
        &mut differences,
        "description",
        json!(left.description),
        json!(right.description),
    );
    push_diff(
        &mut differences,
        "provider",
        json!(left.current_provider),
        json!(right.current_provider),
    );
    push_diff(
        &mut differences,
        "model",
        json!(left.current_model),
        json!(right.current_model),
    );
    push_diff(
        &mut differences,
        "tools",
        json!(left.enabled_tools),
        json!(right.enabled_tools),
    );
    push_diff(
        &mut differences,
        "skills",
        json!(left.enabled_skills),
        json!(right.enabled_skills),
    );
    push_diff(
        &mut differences,
        "mcp_servers",
        json!(left.enabled_mcp_servers),
        json!(right.enabled_mcp_servers),
    );
    push_diff(
        &mut differences,
        "usrl_contracts",
        json!(left.usrl_contracts),
        json!(right.usrl_contracts),
    );
    push_diff(
        &mut differences,
        "cms_user_id",
        json!(left.cms_user_id),
        json!(right.cms_user_id),
    );
    push_diff(
        &mut differences,
        "cms_project_id",
        json!(left.cms_project_id),
        json!(right.cms_project_id),
    );
    push_diff(
        &mut differences,
        "memory_policy",
        json!(left.memory_policy),
        json!(right.memory_policy),
    );
    push_diff(
        &mut differences,
        "status",
        metadata_json(&left, "status"),
        metadata_json(&right, "status"),
    );
    push_diff(
        &mut differences,
        "primary_scope",
        metadata_json(&left, "primary_scope"),
        metadata_json(&right, "primary_scope"),
    );
    push_diff(
        &mut differences,
        "tags",
        metadata_json(&left, "tags"),
        metadata_json(&right, "tags"),
    );
    if prompts {
        push_diff(
            &mut differences,
            "system_prompt",
            json!(left.system_prompt),
            json!(right.system_prompt),
        );
    } else if left.system_prompt != right.system_prompt {
        differences.push(FieldDifference {
                field: "system_prompt".to_string(),
                left: json!({"bytes": left.system_prompt.len(), "sha256": prompt_digest(&left.system_prompt)}),
                right: json!({"bytes": right.system_prompt.len(), "sha256": prompt_digest(&right.system_prompt)}),
            });
    }
    let comparison = AgentComparison {
        left_id: left.id,
        right_id: right.id,
        differences,
    };
    if json_output {
        println!("{}", serde_json::to_string_pretty(&comparison)?);
    } else {
        print_comparison(&comparison);
    }
    Ok(())
}

pub(super) fn history(&self, id: Option<&str>, json_output: bool) -> anyhow::Result<()> {
    let mut events = self.load_history()?;
    if let Some(id) = id {
        let id = normalize_agent_id(id);
        events.retain(|event| normalize_agent_id(&event.agent_id) == id);
    }
    if json_output {
        println!("{}", serde_json::to_string_pretty(&events)?);
    } else if events.is_empty() {
        println!("No history recorded.");
    } else {
        for event in events.iter().rev().take(100) {
            println!(
                "{} {:<18} {} {}",
                event.timestamp, event.agent_id, event.action, event.summary
            );
        }
    }
    Ok(())
}

pub(super) fn status(&self, id: &str, status: String, json_output: bool) -> anyhow::Result<()> {
    let status = normalize_agent_id(&status);
    let allowed = [
        "draft",
        "active",
        "paused",
        "deprecated",
        "archived",
        "broken",
    ];
    if !allowed.contains(&status.as_str()) {
        bail!("status must be one of: {}", allowed.join(","));
    }
    let mut profile = self.store.load(id)?;
    if status == "active" {
        let report = self.validate_profile(&profile)?;
        if !report.errors.is_empty() {
            bail!(
                "refusing to activate {}: validation has {} error(s)",
                profile.id,
                report.errors.len()
            );
        }
    }
    profile
        .metadata
        .insert("status".to_string(), Value::String(status));
    self.save_touched(profile, "status", json_output)
}

pub(super) fn scope(&self, args: ScopeArgs, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(&args.id)?;
    if let Some(primary) = args.primary {
        profile
            .metadata
            .insert("primary_scope".to_string(), Value::String(primary));
    }
    if !args.secondary.is_empty() {
        profile.metadata.insert(
            "secondary_scopes".to_string(),
            json!(clean_list(args.secondary)),
        );
    }
    if let Some(workspace_scope) = args.workspace_scope {
        profile.metadata.insert(
            "workspace_scope".to_string(),
            Value::String(workspace_scope),
        );
    }
    if !args.file_scope.is_empty() {
        profile.metadata.insert(
            "file_scope_hints".to_string(),
            json!(clean_list(args.file_scope)),
        );
    }
    self.save_touched(profile, "scope", json_output)
}

pub(super) fn tags(&self, id: &str, tags: Vec<String>, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    profile
        .metadata
        .insert("tags".to_string(), json!(clean_list(tags)));
    self.save_touched(profile, "tags", json_output)
}

pub(super) fn budget(&self, args: BudgetArgs, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(&args.id)?;
    if args.clear {
        profile.metadata.remove("default_work_budget");
    } else {
        let mut budget = profile
            .metadata
            .get("default_work_budget")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !budget.is_object() {
            budget = json!({});
        }
        let map = budget.as_object_mut().expect("object ensured");
        if let Some(value) = args.max_steps {
            map.insert("max_steps".to_string(), json!(value));
        }
        if let Some(value) = args.max_tool_calls {
            map.insert("max_tool_calls".to_string(), json!(value));
        }
        if let Some(value) = args.max_read_bytes {
            map.insert("max_read_bytes".to_string(), json!(value));
        }
        if let Some(value) = args.max_output_bytes {
            map.insert("max_output_bytes".to_string(), json!(value));
        }
        if !args.allowed_tools.is_empty() {
            map.insert(
                "allowed_tools".to_string(),
                json!(clean_list(args.allowed_tools)),
            );
        }
        if let Some(notes) = args.notes {
            map.insert("notes".to_string(), json!(notes));
        }
        profile
            .metadata
            .insert("default_work_budget".to_string(), budget);
    }
    self.save_touched(profile, "budget", json_output)
}

pub(super) fn templates(&self, id: Option<&str>, json_output: bool) -> anyhow::Result<()> {
    if let Some(id) = id {
        let template = agent_template(id).with_context(|| format!("unknown template: {id}"))?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&template)?);
        } else {
            print_template(&template);
        }
        return Ok(());
    }
    let templates = agent_templates();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&templates)?);
    } else {
        for template in templates {
            println!(
                "{:<14} {:<22} tools={} skills={}",
                template.mode,
                template.display_name,
                list_or_dash(&template.enabled_tools),
                list_or_dash(&template.enabled_skills)
            );
        }
    }
    Ok(())
}

pub(super) fn list(&self, args: ListArgs, json_output: bool) -> anyhow::Result<()> {
    let (mut profiles, warnings) = self.store.list_lossy()?;
    if let Some(mode) = args.mode {
        let mode = normalize_agent_id(&mode);
        profiles.retain(|profile| normalize_agent_id(&profile.mode) == mode);
    }
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "agents_root": self.store.root,
                "profiles": profiles,
                "warnings": warnings,
            }))?
        );
        return Ok(());
    }
    if profiles.is_empty() {
        println!("No agents found in {}", self.store.root.display());
    } else {
        for profile in profiles {
            println!(
                "{:<24} mode={:<14} name={} provider={} model={} tools={} skills={}",
                profile.id,
                profile.mode,
                profile.display_name,
                profile.current_provider.as_deref().unwrap_or("-"),
                profile.current_model.as_deref().unwrap_or("-"),
                list_or_dash(&profile.enabled_tools),
                list_or_dash(&profile.enabled_skills),
            );
            if args.long {
                println!("  description: {}", dash_if_empty(&profile.description));
                println!("  cms: {}/{}", profile.cms_user_id, profile.cms_project_id);
                println!("  mcp: {}", list_or_dash(&profile.enabled_mcp_servers));
                println!("  usrl: {}", list_or_dash(&profile.usrl_contracts));
                println!("  memory_policy: {}", profile.memory_policy);
                println!("  prompt_bytes: {}", profile.system_prompt.len());
            }
        }
    }
    if !warnings.is_empty() {
        println!("\nWarnings:");
        for warning in warnings {
            println!("- {warning}");
        }
    }
    Ok(())
}

pub(super) fn show(&self, id: &str, json_output: bool) -> anyhow::Result<()> {
    let profile = self.store.load(id)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&profile)?);
    } else {
        print_profile(&profile);
    }
    Ok(())
}

pub(super) fn create(&self, args: CreateArgs, json_output: bool) -> anyhow::Result<()> {
    let id = normalize_agent_id(&args.id);
    if id.is_empty() {
        bail!("agent id must contain at least one letter or number");
    }
    self.ensure_can_write(&id, args.force)?;

    let mut profile = if let Some(template_id) = &args.template {
        profile_from_template(template_id, &id, args.name.as_deref())?
    } else {
        let prompt = read_prompt(args.prompt.clone(), args.prompt_file.clone())?.unwrap_or_else(|| {
                format!(
                    "You are {name}, a Vegvisir custom agent. Work evidence-first, preserve user work, follow tool and secret boundaries, and report verification clearly.",
                    name = args.name.as_deref().unwrap_or(&id)
                )
            });
        AgentProfile::new(&id, args.name.clone().unwrap_or_else(|| id.clone()), prompt)?
    };

    if args.template.is_none() || args.mode != "custom" {
        profile.mode = normalized_or_default(&args.mode, "custom");
    }
    if let Some(name) = args.name {
        profile.display_name = name;
    }
    if let Some(description) = args.description {
        profile.description = description;
    }
    if let Some(prompt) = read_prompt(args.prompt, args.prompt_file)? {
        profile.system_prompt = prompt;
    }
    profile.current_provider = args.provider.and_then(none_marker);
    profile.current_model = args.model.and_then(none_marker);
    if !args.tools.is_empty() {
        let tools = clean_list(args.tools);
        if args.add_tools {
            append_unique(&mut profile.enabled_tools, tools);
        } else {
            profile.enabled_tools = tools;
        }
    }
    if !args.skills.is_empty() {
        let skills = clean_list(args.skills);
        if args.add_skills {
            append_unique(&mut profile.enabled_skills, skills);
        } else {
            profile.enabled_skills = skills;
        }
    }
    if !args.mcp.is_empty() {
        profile.enabled_mcp_servers = clean_list(args.mcp);
    }
    if !args.usrl.is_empty() {
        profile.usrl_contracts = clean_list(args.usrl);
    }
    if let Some(policy) = args.memory_policy.filter(|value| !value.trim().is_empty()) {
        profile.memory_policy = policy;
    }
    touch_metadata(&mut profile, "created");
    let path = self.store.save(&profile)?;
    self.append_history(&profile, "created", &path)?;
    print_saved(&profile, &path, json_output)
}

pub(super) fn create_template(
    &self,
    args: CreateTemplateArgs,
    json_output: bool,
) -> anyhow::Result<()> {
    let id = normalize_agent_id(&args.id);
    if id.is_empty() {
        bail!("agent id must contain at least one letter or number");
    }
    self.ensure_can_write(&id, args.force)?;
    let mut profile = profile_from_template(&args.mode, &id, args.name.as_deref())?;
    if let Some(description) = args.description {
        profile.description = description;
    }
    touch_metadata(&mut profile, "created-from-template");
    let path = self.store.save(&profile)?;
    self.append_history(&profile, "created-from-template", &path)?;
    print_saved(&profile, &path, json_output)
}

pub(super) fn design(&self, args: DesignArgs, json_output: bool) -> anyhow::Result<()> {
    let prompt = read_prompt(args.prompt, args.prompt_file)?
        .with_context(|| "design requires --prompt or --prompt-file")?;
    let create_args = CreateArgs {
        id: args.id,
        template: agent_template(&args.mode).map(|_| args.mode.clone()),
        mode: args.mode,
        name: Some(args.name),
        description: args.description,
        prompt: Some(prompt),
        prompt_file: None,
        provider: args.provider,
        model: args.model,
        tools: args.tools,
        add_tools: false,
        skills: args.skills,
        add_skills: false,
        mcp: args.mcp,
        usrl: args.usrl,
        memory_policy: args.memory_policy,
        force: args.force,
    };
    self.create(create_args, json_output)
}

pub(super) fn set(&self, args: SetArgs, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(&args.id)?;
    if let Some(mode) = args.mode {
        profile.mode = normalized_or_default(&mode, "custom");
    }
    if let Some(name) = args.name {
        profile.display_name = name;
    }
    if let Some(description) = args.description {
        profile.description = description;
    }
    if let Some(prompt) = read_prompt(args.prompt, args.prompt_file)? {
        profile.system_prompt = prompt;
    }
    if let Some(provider) = args.provider {
        profile.current_provider = none_marker(provider);
        if profile.current_provider.is_none() {
            profile.current_model = None;
        }
    }
    if let Some(model) = args.model {
        profile.current_model = none_marker(model);
    }
    if let Some(tools) = args.tools {
        profile.enabled_tools = clean_list(tools);
    }
    append_unique(&mut profile.enabled_tools, clean_list(args.add_tools));
    remove_all(&mut profile.enabled_tools, &clean_list(args.remove_tools));
    if let Some(skills) = args.skills {
        profile.enabled_skills = clean_list(skills);
    }
    append_unique(&mut profile.enabled_skills, clean_list(args.add_skills));
    remove_all(&mut profile.enabled_skills, &clean_list(args.remove_skills));
    if let Some(mcp) = args.mcp {
        profile.enabled_mcp_servers = clean_list(mcp);
    }
    append_unique(&mut profile.enabled_mcp_servers, clean_list(args.add_mcp));
    remove_all(
        &mut profile.enabled_mcp_servers,
        &clean_list(args.remove_mcp),
    );
    if let Some(usrl) = args.usrl {
        profile.usrl_contracts = clean_list(usrl);
    }
    append_unique(&mut profile.usrl_contracts, clean_list(args.add_usrl));
    remove_all(&mut profile.usrl_contracts, &clean_list(args.remove_usrl));
    if let Some(policy) = args.memory_policy {
        profile.memory_policy = policy;
    }
    match (args.cms_user, args.cms_project) {
        (Some(user), Some(project)) => {
            profile.cms_user_id = user;
            profile.cms_project_id = project;
        }
        (None, None) => {}
        _ => bail!("--cms-user and --cms-project must be passed together"),
    }
    self.save_touched(profile, "set", json_output)
}

pub(super) fn name(&self, id: &str, name: String, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    profile.display_name = name;
    self.save_touched(profile, "name", json_output)
}

pub(super) fn mode(&self, id: &str, mode: String, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    profile.mode = normalized_or_default(&mode, "custom");
    self.save_touched(profile, "mode", json_output)
}

pub(super) fn describe(
    &self,
    id: &str,
    description: String,
    json_output: bool,
) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    profile.description = description;
    self.save_touched(profile, "describe", json_output)
}

pub(super) fn provider(&self, id: &str, provider: String, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    profile.current_provider = none_marker(provider);
    if profile.current_provider.is_none() {
        profile.current_model = None;
    }
    self.save_touched(profile, "provider", json_output)
}

pub(super) fn model(&self, id: &str, model: String, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    profile.current_model = none_marker(model);
    self.save_touched(profile, "model", json_output)
}

pub(super) fn prompt(&self, args: PromptArgs, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(&args.id)?;
    let prompt = if let Some(path) = args.prompt_file {
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?
    } else {
        join_required("prompt", args.prompt)?
    };
    profile.system_prompt = prompt;
    self.save_touched(profile, "prompt", json_output)
}

pub(super) fn add_to_list(
    &self,
    id: &str,
    field: ListField,
    value: String,
    json_output: bool,
) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    append_unique(field.get_mut(&mut profile), clean_list(vec![value]));
    self.save_touched(profile, &format!("add-{}", field.label()), json_output)
}

pub(super) fn remove_from_list(
    &self,
    id: &str,
    field: ListField,
    value: &str,
    json_output: bool,
) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    remove_all(
        field.get_mut(&mut profile),
        &clean_list(vec![value.to_string()]),
    );
    self.save_touched(profile, &format!("remove-{}", field.label()), json_output)
}

pub(super) fn replace_list(
    &self,
    id: &str,
    field: ListField,
    values: Vec<String>,
    json_output: bool,
) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    *field.get_mut(&mut profile) = clean_list(values);
    self.save_touched(profile, &format!("set-{}", field.label()), json_output)
}

pub(super) fn memory_policy(
    &self,
    id: &str,
    policy: String,
    json_output: bool,
) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    profile.memory_policy = policy;
    self.save_touched(profile, "memory-policy", json_output)
}

pub(super) fn cms_scope(
    &self,
    id: &str,
    user: String,
    project: String,
    json_output: bool,
) -> anyhow::Result<()> {
    if user.trim().is_empty() || project.trim().is_empty() {
        bail!("CMS user and project ids must be non-empty");
    }
    let mut profile = self.store.load(id)?;
    profile.cms_user_id = user;
    profile.cms_project_id = project;
    self.save_touched(profile, "cms-scope", json_output)
}

pub(super) fn reset_cms_scope(&self, id: &str, json_output: bool) -> anyhow::Result<()> {
    let mut profile = self.store.load(id)?;
    let scope = format!("agent:{}", profile.id);
    profile.cms_user_id = scope.clone();
    profile.cms_project_id = scope;
    self.save_touched(profile, "reset-cms-scope", json_output)
}

pub(super) fn clone_profile(
    &self,
    source_id: &str,
    new_id: &str,
    name: Option<String>,
    force: bool,
    json_output: bool,
) -> anyhow::Result<()> {
    let mut profile = self.store.load(source_id)?;
    let normalized = normalize_agent_id(new_id);
    if normalized.is_empty() {
        bail!("new agent id must contain at least one letter or number");
    }
    self.ensure_can_write(&normalized, force)?;
    profile.id = normalized.clone();
    if let Some(name) = name {
        profile.display_name = name;
    }
    let cms_scope = format!("agent:{normalized}");
    profile.cms_user_id = cms_scope.clone();
    profile.cms_project_id = cms_scope;
    profile.created_at = chrono::Utc::now();
    profile.updated_at = profile.created_at;
    profile.metadata = admin_metadata("cloned");
    profile.metadata.insert(
        "cloned_from".to_string(),
        Value::String(source_id.to_string()),
    );
    let path = self.store.save(&profile)?;
    self.append_history(&profile, "cloned", &path)?;
    print_saved(&profile, &path, json_output)
}

pub(super) fn delete(&self, id: &str, yes: bool, json_output: bool) -> anyhow::Result<()> {
    if !yes {
        bail!("refusing to delete {id} without --yes");
    }
    let path = self.store.delete(id)?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "deleted": id,
                "path": path,
            }))?
        );
    } else {
        println!("Deleted agent {id} at {}", path.display());
    }
    Ok(())
}

pub(super) fn export(&self, id: &str, out: Option<PathBuf>) -> anyhow::Result<()> {
    let profile = self.store.load(id)?;
    let text = serde_json::to_string_pretty(&profile)?;
    if let Some(path) = out {
        std::fs::write(&path, text)?;
        println!("Exported agent {} to {}", profile.id, path.display());
    } else {
        println!("{text}");
    }
    Ok(())
}

pub(super) fn import(&self, path: &PathBuf, force: bool, json_output: bool) -> anyhow::Result<()> {
    let mut profile: AgentProfile = serde_json::from_str(
        &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
    )?;
    profile.id = normalize_agent_id(&profile.id);
    if profile.id.is_empty() {
        bail!("imported agent id must contain at least one letter or number");
    }
    self.ensure_can_write(&profile.id, force)?;
    profile.updated_at = chrono::Utc::now();
    profile.metadata.insert(
        "last_admin_action".to_string(),
        Value::String("imported".to_string()),
    );
    profile.metadata.insert(
        "imported_from".to_string(),
        Value::String(path.display().to_string()),
    );
    let saved = self.store.save(&profile)?;
    self.append_history(&profile, "imported", &saved)?;
    print_saved(&profile, &saved, json_output)
}

pub(super) fn tui(&self) -> anyhow::Result<()> {
    run_admin_tui(self)
}

}

const TUI_BG: Color = Color::Rgb(8, 9, 10);
const TUI_FG: Color = Color::Rgb(220, 220, 220);
const TUI_DIM: Color = Color::Rgb(105, 105, 112);
const TUI_GREEN: Color = Color::Rgb(88, 220, 120);
const TUI_CYAN: Color = Color::Rgb(80, 190, 220);
const TUI_AMBER: Color = Color::Rgb(220, 170, 65);
const TUI_RED: Color = Color::Rgb(230, 86, 86);
const TUI_BORDER: Color = Color::Rgb(62, 66, 76);
const TUI_PANEL: Color = Color::Rgb(16, 17, 20);

#[derive(Default)]
struct AdminTuiState {
    selected: usize,
    mode: AdminTuiMode,
    show_help: bool,
    filter: String,
    input: String,
    action_selected: usize,
    message: String,
}

#[derive(Default, Copy, Clone, Eq, PartialEq)]
enum AdminTuiMode {
    #[default]
    Browse,
    Search,
    ActionMenu,
    TagsInput,
    PrimaryScopeInput,
    SecondaryScopesInput,
    WorkspaceScopeInput,
    FileScopeInput,
    MemoryPolicyInput,
    BudgetMaxStepsInput,
    BudgetMaxToolCallsInput,
    BudgetMaxReadBytesInput,
    BudgetMaxOutputBytesInput,
    BudgetAllowedToolsInput,
    BudgetNotesInput,
    ProviderInput,
    ModelInput,
    ToolsInput,
    SkillsInput,
    McpInput,
    UsrlInput,
}

#[derive(Copy, Clone)]
enum TuiAction {
    Validate,
    Metrics,
    History,
    Activate,
    Pause,
    Deprecate,
    Archive,
    EditProvider,
    ClearProvider,
    EditModel,
    ClearModel,
    EditTools,
    ClearTools,
    EditSkills,
    ClearSkills,
    EditMcp,
    ClearMcp,
    EditUsrl,
    ClearUsrl,
    EditPrimaryScope,
    EditSecondaryScopes,
    EditWorkspaceScope,
    EditFileScope,
    ClearScopeMetadata,
    EditMemoryPolicy,
    EditBudgetMaxSteps,
    EditBudgetMaxToolCalls,
    EditBudgetMaxReadBytes,
    EditBudgetMaxOutputBytes,
    EditBudgetAllowedTools,
    EditBudgetNotes,
    ClearBudget,
    EditTags,
    ClearTags,
}

impl TuiAction {
    fn label(self) -> &'static str {
        match self {
            Self::Validate => "Validate selected agent",
            Self::Metrics => "Show selected metrics summary",
            Self::History => "Show selected history count",
            Self::Activate => "Set status: active",
            Self::Pause => "Set status: paused",
            Self::Deprecate => "Set status: deprecated",
            Self::Archive => "Set status: archived",
            Self::EditProvider => "Edit provider",
            Self::ClearProvider => "Clear provider and model",
            Self::EditModel => "Edit model",
            Self::ClearModel => "Clear model",
            Self::EditTools => "Edit tool allow-list",
            Self::ClearTools => "Clear tool allow-list",
            Self::EditSkills => "Edit enabled skills",
            Self::ClearSkills => "Clear enabled skills",
            Self::EditMcp => "Edit allowed MCP servers",
            Self::ClearMcp => "Clear allowed MCP servers",
            Self::EditUsrl => "Edit bound USRL contracts",
            Self::ClearUsrl => "Clear bound USRL contracts",
            Self::EditPrimaryScope => "Edit primary scope",
            Self::EditSecondaryScopes => "Edit secondary scopes",
            Self::EditWorkspaceScope => "Edit workspace scope",
            Self::EditFileScope => "Edit file-scope hints",
            Self::ClearScopeMetadata => "Clear scope metadata",
            Self::EditMemoryPolicy => "Edit memory policy",
            Self::EditBudgetMaxSteps => "Edit budget max steps",
            Self::EditBudgetMaxToolCalls => "Edit budget max tool calls",
            Self::EditBudgetMaxReadBytes => "Edit budget max read bytes",
            Self::EditBudgetMaxOutputBytes => "Edit budget max output bytes",
            Self::EditBudgetAllowedTools => "Edit budget allowed tools",
            Self::EditBudgetNotes => "Edit budget notes",
            Self::ClearBudget => "Clear default work budget",
            Self::EditTags => "Edit tags",
            Self::ClearTags => "Clear tags",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::Validate => "Run validation and summarize errors/warnings.",
            Self::Metrics => "Load the agent metrics file and show task success.",
            Self::History => "Count recorded admin history events for this agent.",
            Self::Activate => "Mark active after validation passes without hard errors.",
            Self::Pause => "Mark paused without deleting or changing permissions.",
            Self::Deprecate => "Mark deprecated for operators while retaining the profile.",
            Self::Archive => "Mark archived for old or inactive profiles.",
            Self::EditProvider => "Open a provider editor; use '-' or 'clear' to inherit.",
            Self::ClearProvider => {
                "Clear explicit provider and model so runtime inheritance applies."
            }
            Self::EditModel => "Open a model editor; model/provider compatibility is checked.",
            Self::ClearModel => "Clear the explicit model while preserving the provider.",
            Self::EditTools => {
                "Replace the comma-separated tool allow-list after catalog validation."
            }
            Self::ClearTools => "Remove all explicitly enabled tools from the selected profile.",
            Self::EditSkills => {
                "Replace the comma-separated enabled skills after catalog validation."
            }
            Self::ClearSkills => "Remove all enabled skills from the selected profile.",
            Self::EditMcp => {
                "Replace the comma-separated MCP server allow-list after mcp.json validation."
            }
            Self::ClearMcp => "Remove all allowed MCP servers from the selected profile.",
            Self::EditUsrl => {
                "Replace bound USRL contract refs; USRL skill refs are expanded when known."
            }
            Self::ClearUsrl => "Remove all bound USRL contract refs from the selected profile.",
            Self::EditPrimaryScope => "Set the primary operator scope stored in profile metadata.",
            Self::EditSecondaryScopes => "Replace comma-separated secondary scope metadata.",
            Self::EditWorkspaceScope => "Set the workspace/repository scope metadata label.",
            Self::EditFileScope => "Replace comma-separated file-scope hint metadata.",
            Self::ClearScopeMetadata => {
                "Remove primary, secondary, workspace, and file-scope metadata."
            }
            Self::EditMemoryPolicy => {
                "Set the agent memory policy label; empty, '-' or clear resets to agent-scoped."
            }
            Self::EditBudgetMaxSteps => {
                "Set or clear default max_steps for future subagent work budgets."
            }
            Self::EditBudgetMaxToolCalls => {
                "Set or clear default max_tool_calls for future subagent work budgets."
            }
            Self::EditBudgetMaxReadBytes => {
                "Set or clear default max_read_bytes for future subagent work budgets."
            }
            Self::EditBudgetMaxOutputBytes => {
                "Set or clear default max_output_bytes for future subagent work budgets."
            }
            Self::EditBudgetAllowedTools => {
                "Replace default budget allowed_tools after tool catalog validation."
            }
            Self::EditBudgetNotes => "Set or clear default work-budget notes.",
            Self::ClearBudget => "Remove all default work-budget metadata from this profile.",
            Self::EditTags => "Open a comma-separated tag editor for the selected profile.",
            Self::ClearTags => "Remove all tag metadata from the selected profile.",
        }
    }
}

const TUI_ACTIONS: &[TuiAction] = &[
    TuiAction::Validate,
    TuiAction::Metrics,
    TuiAction::History,
    TuiAction::Activate,
    TuiAction::Pause,
    TuiAction::Deprecate,
    TuiAction::Archive,
    TuiAction::EditProvider,
    TuiAction::ClearProvider,
    TuiAction::EditModel,
    TuiAction::ClearModel,
    TuiAction::EditTools,
    TuiAction::ClearTools,
    TuiAction::EditSkills,
    TuiAction::ClearSkills,
    TuiAction::EditMcp,
    TuiAction::ClearMcp,
    TuiAction::EditUsrl,
    TuiAction::ClearUsrl,
    TuiAction::EditPrimaryScope,
    TuiAction::EditSecondaryScopes,
    TuiAction::EditWorkspaceScope,
    TuiAction::EditFileScope,
    TuiAction::ClearScopeMetadata,
    TuiAction::EditMemoryPolicy,
    TuiAction::EditBudgetMaxSteps,
    TuiAction::EditBudgetMaxToolCalls,
    TuiAction::EditBudgetMaxReadBytes,
    TuiAction::EditBudgetMaxOutputBytes,
    TuiAction::EditBudgetAllowedTools,
    TuiAction::EditBudgetNotes,
    TuiAction::ClearBudget,
    TuiAction::EditTags,
    TuiAction::ClearTags,
];

fn run_admin_tui(admin: &AgentRegistryAdmin) -> anyhow::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("vegvisir-agent-admin tui requires an interactive terminal");
    }
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_admin_tui_inner(admin, &mut terminal);
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show);
    let _ = terminal.show_cursor();
    result
}

fn run_admin_tui_inner(
    admin: &AgentRegistryAdmin,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    let mut state = AdminTuiState {
        message: "F/Ctrl+F search, F2/A actions, E scope, Y memory, B budget, P provider, O model, U tools, S skills, D MCP, L USRL, T tags, F1 help"
            .to_string(),
        ..Default::default()
    };
    loop {
        let all_profiles = admin.store.list_lossy()?.0;
        let profiles = filtered_profiles(&all_profiles, &state.filter);
        if profiles.is_empty() {
            state.selected = 0;
        } else if state.selected >= profiles.len() {
            state.selected = profiles.len() - 1;
        }
        let selected_id = profiles
            .get(state.selected)
            .map(|profile| profile.id.as_str());
        terminal.draw(|frame| draw_admin_tui(frame, &profiles, selected_id, &state))?;
        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(key) = event::read()? {
                if handle_admin_tui_key(admin, &mut state, &profiles, key)? {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn handle_admin_tui_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    if state.show_help {
        match key.code {
            KeyCode::Esc | KeyCode::F(1) => {
                state.show_help = false;
                state.message = "help hidden".to_string();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(true);
            }
            _ => {}
        }
        return Ok(false);
    }

    match state.mode {
        AdminTuiMode::Search => return handle_admin_tui_search_key(state, key),
        AdminTuiMode::ActionMenu => {
            return handle_admin_tui_action_key(admin, state, profiles, key);
        }
        AdminTuiMode::TagsInput => {
            return handle_admin_tui_tags_key(admin, state, profiles, key);
        }
        AdminTuiMode::PrimaryScopeInput => {
            return handle_admin_tui_primary_scope_key(admin, state, profiles, key);
        }
        AdminTuiMode::SecondaryScopesInput => {
            return handle_admin_tui_secondary_scopes_key(admin, state, profiles, key);
        }
        AdminTuiMode::WorkspaceScopeInput => {
            return handle_admin_tui_workspace_scope_key(admin, state, profiles, key);
        }
        AdminTuiMode::FileScopeInput => {
            return handle_admin_tui_file_scope_key(admin, state, profiles, key);
        }
        AdminTuiMode::MemoryPolicyInput => {
            return handle_admin_tui_memory_policy_key(admin, state, profiles, key);
        }
        AdminTuiMode::BudgetMaxStepsInput => {
            return handle_admin_tui_budget_number_key(
                admin,
                state,
                profiles,
                key,
                "max_steps",
                "max steps",
            );
        }
        AdminTuiMode::BudgetMaxToolCallsInput => {
            return handle_admin_tui_budget_number_key(
                admin,
                state,
                profiles,
                key,
                "max_tool_calls",
                "max tool calls",
            );
        }
        AdminTuiMode::BudgetMaxReadBytesInput => {
            return handle_admin_tui_budget_number_key(
                admin,
                state,
                profiles,
                key,
                "max_read_bytes",
                "max read bytes",
            );
        }
        AdminTuiMode::BudgetMaxOutputBytesInput => {
            return handle_admin_tui_budget_number_key(
                admin,
                state,
                profiles,
                key,
                "max_output_bytes",
                "max output bytes",
            );
        }
        AdminTuiMode::BudgetAllowedToolsInput => {
            return handle_admin_tui_budget_allowed_tools_key(admin, state, profiles, key);
        }
        AdminTuiMode::BudgetNotesInput => {
            return handle_admin_tui_budget_notes_key(admin, state, profiles, key);
        }
        AdminTuiMode::ProviderInput => {
            return handle_admin_tui_provider_key(admin, state, profiles, key);
        }
        AdminTuiMode::ModelInput => {
            return handle_admin_tui_model_key(admin, state, profiles, key);
        }
        AdminTuiMode::ToolsInput => {
            return handle_admin_tui_tools_key(admin, state, profiles, key);
        }
        AdminTuiMode::SkillsInput => {
            return handle_admin_tui_skills_key(admin, state, profiles, key);
        }
        AdminTuiMode::McpInput => {
            return handle_admin_tui_mcp_key(admin, state, profiles, key);
        }
        AdminTuiMode::UsrlInput => {
            return handle_admin_tui_usrl_key(admin, state, profiles, key);
        }
        AdminTuiMode::Browse => {}
    }

    match key.code {
        KeyCode::Esc => return Ok(true),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Char('f') | KeyCode::Char('F') | KeyCode::F(3)
            if key.modifiers.is_empty()
                || key.modifiers == KeyModifiers::SHIFT
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            state.mode = AdminTuiMode::Search;
            state.message = "type to search agents; Enter applies, Esc cancels".to_string();
        }
        KeyCode::F(2) | KeyCode::Char('a') | KeyCode::Char('A') => {
            state.mode = AdminTuiMode::ActionMenu;
            state.action_selected = state
                .action_selected
                .min(TUI_ACTIONS.len().saturating_sub(1));
            state.message = "choose an action; Enter applies, Esc cancels".to_string();
        }
        KeyCode::Char('e') | KeyCode::Char('E') => begin_primary_scope_input(state, profiles),
        KeyCode::Char('y') | KeyCode::Char('Y') => begin_memory_policy_input(state, profiles),
        KeyCode::Char('b') | KeyCode::Char('B') => begin_budget_max_steps_input(state, profiles),
        KeyCode::Char('p') | KeyCode::Char('P') => begin_provider_input(state, profiles),
        KeyCode::Char('o') | KeyCode::Char('O') => begin_model_input(state, profiles),
        KeyCode::Char('u') | KeyCode::Char('U') => begin_tools_input(state, profiles),
        KeyCode::Char('s') | KeyCode::Char('S') => begin_skills_input(state, profiles),
        KeyCode::Char('d') | KeyCode::Char('D') => begin_mcp_input(state, profiles),
        KeyCode::Char('l') | KeyCode::Char('L') => begin_usrl_input(state, profiles),
        KeyCode::Char('t') | KeyCode::Char('T') => begin_tags_input(state, profiles),
        KeyCode::Char('r') | KeyCode::Char('R') => state.message = "refreshed".to_string(),
        KeyCode::F(1) => {
            state.show_help = true;
            state.message = "help shown".to_string();
        }
        KeyCode::Enter | KeyCode::Char('v') | KeyCode::Char('V') => {
            apply_tui_action(admin, state, profiles, TuiAction::Validate)?;
        }
        KeyCode::Char('m') | KeyCode::Char('M') => {
            apply_tui_action(admin, state, profiles, TuiAction::Metrics)?;
        }
        KeyCode::Char('h') | KeyCode::Char('H') => {
            apply_tui_action(admin, state, profiles, TuiAction::History)?;
        }
        KeyCode::Up => state.selected = state.selected.saturating_sub(1),
        KeyCode::Down => {
            if state.selected + 1 < profiles.len() {
                state.selected += 1;
            }
        }
        KeyCode::Home => state.selected = 0,
        KeyCode::End => {
            if !profiles.is_empty() {
                state.selected = profiles.len() - 1;
            }
        }
        KeyCode::PageUp => state.selected = state.selected.saturating_sub(5),
        KeyCode::PageDown => {
            if !profiles.is_empty() {
                state.selected = (state.selected + 5).min(profiles.len() - 1);
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_search_key(state: &mut AdminTuiState, key: KeyEvent) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = if state.filter.is_empty() {
                "search cancelled".to_string()
            } else {
                format!("search: {}", state.filter)
            };
        }
        KeyCode::Enter => {
            state.mode = AdminTuiMode::Browse;
            state.selected = 0;
            state.message = if state.filter.is_empty() {
                "search cleared".to_string()
            } else {
                format!("search applied: {}", state.filter)
            };
        }
        KeyCode::Backspace => {
            state.filter.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.filter.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_action_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "action cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Up => state.action_selected = state.action_selected.saturating_sub(1),
        KeyCode::Down => {
            if state.action_selected + 1 < TUI_ACTIONS.len() {
                state.action_selected += 1;
            }
        }
        KeyCode::Home => state.action_selected = 0,
        KeyCode::End => state.action_selected = TUI_ACTIONS.len().saturating_sub(1),
        KeyCode::Enter => {
            let action = TUI_ACTIONS
                .get(state.action_selected)
                .copied()
                .unwrap_or(TuiAction::Validate);
            apply_tui_action(admin, state, profiles, action)?;
            if state.mode == AdminTuiMode::ActionMenu {
                state.mode = AdminTuiMode::Browse;
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_tags_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "tag edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let tags = clean_list(vec![state.input.clone()]);
                tui_set_tags(admin, &profile.id, tags)?;
                state.message = format!("tags updated for {}", profile.id);
            } else {
                state.message = "no selected agent to tag".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_primary_scope_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "primary scope edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                tui_set_metadata_string(
                    admin,
                    &profile.id,
                    "primary_scope",
                    none_marker(state.input.clone()),
                    "tui-scope",
                )?;
                state.message = format!("primary scope updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_secondary_scopes_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "secondary scope edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                tui_set_metadata_list(
                    admin,
                    &profile.id,
                    "secondary_scopes",
                    clean_list(vec![state.input.clone()]),
                    "tui-scope",
                )?;
                state.message = format!("secondary scopes updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_workspace_scope_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "workspace scope edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                tui_set_metadata_string(
                    admin,
                    &profile.id,
                    "workspace_scope",
                    none_marker(state.input.clone()),
                    "tui-scope",
                )?;
                state.message = format!("workspace scope updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_file_scope_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "file-scope hint edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                tui_set_metadata_list(
                    admin,
                    &profile.id,
                    "file_scope_hints",
                    clean_list(vec![state.input.clone()]),
                    "tui-scope",
                )?;
                state.message = format!("file-scope hints updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_memory_policy_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "memory policy edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let policy =
                    none_marker(state.input.clone()).unwrap_or_else(|| "agent-scoped".to_string());
                tui_set_memory_policy(admin, &profile.id, policy.clone())?;
                state.message = format!("memory policy {}: {}", profile.id, policy);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_budget_number_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
    budget_key: &str,
    label: &str,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = format!("budget {label} edit cancelled");
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let value = parse_optional_u64_input(&state.input, label)?;
                tui_set_budget_u64(admin, &profile.id, budget_key, value)?;
                state.message = match value {
                    Some(value) => format!("budget {label} {}: {}", profile.id, value),
                    None => format!("budget {label} cleared for {}", profile.id),
                };
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_budget_allowed_tools_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "budget allowed-tools edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let tools = clean_list(vec![state.input.clone()]);
                tui_set_budget_allowed_tools(admin, &profile.id, tools)?;
                state.message = format!("budget allowed tools updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_budget_notes_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "budget notes edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                tui_set_budget_notes(admin, &profile.id, none_marker(state.input.clone()))?;
                state.message = format!("budget notes updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_provider_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "provider edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let provider = none_marker(state.input.clone());
                tui_set_provider(admin, &profile.id, provider.clone())?;
                state.message = match provider {
                    Some(provider) => format!("provider {}: {}", profile.id, provider),
                    None => format!("provider/model cleared for {}", profile.id),
                };
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_model_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "model edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let model = none_marker(state.input.clone());
                tui_set_model(admin, &profile.id, model.clone())?;
                state.message = match model {
                    Some(model) => format!("model {}: {}", profile.id, model),
                    None => format!("model cleared for {}", profile.id),
                };
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_tools_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "tool edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let tools = clean_list(vec![state.input.clone()]);
                tui_set_tools(admin, &profile.id, tools)?;
                state.message = format!("tools updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_skills_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "skill edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let skills = clean_list(vec![state.input.clone()]);
                tui_set_skills(admin, &profile.id, skills)?;
                state.message = format!("skills updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_mcp_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "MCP edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let servers = clean_list(vec![state.input.clone()]);
                tui_set_mcp_servers(admin, &profile.id, servers)?;
                state.message = format!("MCP servers updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_admin_tui_usrl_key(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: KeyEvent,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            state.mode = AdminTuiMode::Browse;
            state.message = "USRL edit cancelled".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Enter => {
            if let Some(profile) = profiles.get(state.selected) {
                let contracts = resolve_usrl_contract_refs_for_admin(
                    admin,
                    clean_list(vec![state.input.clone()]),
                )?;
                tui_set_usrl_contracts(admin, &profile.id, contracts)?;
                state.message = format!("USRL contracts updated for {}", profile.id);
            } else {
                state.message = "no selected agent to edit".to_string();
            }
            state.mode = AdminTuiMode::Browse;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.input.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn begin_tags_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile_tags(profile).join(", ");
        state.mode = AdminTuiMode::TagsInput;
        state.message = format!("editing tags for {}", profile.id);
    } else {
        state.message = "no selected agent to tag".to_string();
    }
}

fn begin_primary_scope_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile
            .metadata
            .get("primary_scope")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        state.mode = AdminTuiMode::PrimaryScopeInput;
        state.message = format!("editing primary scope for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_secondary_scopes_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = metadata_string_list(profile, "secondary_scopes").join(", ");
        state.mode = AdminTuiMode::SecondaryScopesInput;
        state.message = format!("editing secondary scopes for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_workspace_scope_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile
            .metadata
            .get("workspace_scope")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        state.mode = AdminTuiMode::WorkspaceScopeInput;
        state.message = format!("editing workspace scope for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_file_scope_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = metadata_string_list(profile, "file_scope_hints").join(", ");
        state.mode = AdminTuiMode::FileScopeInput;
        state.message = format!("editing file-scope hints for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_memory_policy_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile.memory_policy.clone();
        state.mode = AdminTuiMode::MemoryPolicyInput;
        state.message = format!("editing memory policy for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_budget_max_steps_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    begin_budget_number_input(
        state,
        profiles,
        "max_steps",
        AdminTuiMode::BudgetMaxStepsInput,
        "max steps",
    );
}

fn begin_budget_max_tool_calls_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    begin_budget_number_input(
        state,
        profiles,
        "max_tool_calls",
        AdminTuiMode::BudgetMaxToolCallsInput,
        "max tool calls",
    );
}

fn begin_budget_max_read_bytes_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    begin_budget_number_input(
        state,
        profiles,
        "max_read_bytes",
        AdminTuiMode::BudgetMaxReadBytesInput,
        "max read bytes",
    );
}

fn begin_budget_max_output_bytes_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    begin_budget_number_input(
        state,
        profiles,
        "max_output_bytes",
        AdminTuiMode::BudgetMaxOutputBytesInput,
        "max output bytes",
    );
}

fn begin_budget_number_input(
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    key: &str,
    mode: AdminTuiMode,
    label: &str,
) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = budget_u64(profile, key)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        state.mode = mode;
        state.message = format!("editing budget {label} for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_budget_allowed_tools_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = budget_string_list(profile, "allowed_tools").join(", ");
        state.mode = AdminTuiMode::BudgetAllowedToolsInput;
        state.message = format!("editing budget allowed tools for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_budget_notes_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = budget_string(profile, "notes")
            .map(str::to_string)
            .unwrap_or_else(|| "-".to_string());
        state.mode = AdminTuiMode::BudgetNotesInput;
        state.message = format!("editing budget notes for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_provider_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile
            .current_provider
            .clone()
            .unwrap_or_else(|| "-".to_string());
        state.mode = AdminTuiMode::ProviderInput;
        state.message = format!("editing provider for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_model_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile
            .current_model
            .clone()
            .unwrap_or_else(|| "-".to_string());
        state.mode = AdminTuiMode::ModelInput;
        state.message = format!("editing model for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_tools_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile.enabled_tools.join(", ");
        state.mode = AdminTuiMode::ToolsInput;
        state.message = format!("editing tools for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_skills_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile.enabled_skills.join(", ");
        state.mode = AdminTuiMode::SkillsInput;
        state.message = format!("editing skills for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_mcp_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile.enabled_mcp_servers.join(", ");
        state.mode = AdminTuiMode::McpInput;
        state.message = format!("editing MCP servers for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn begin_usrl_input(state: &mut AdminTuiState, profiles: &[AgentProfile]) {
    if let Some(profile) = profiles.get(state.selected) {
        state.input = profile.usrl_contracts.join(", ");
        state.mode = AdminTuiMode::UsrlInput;
        state.message = format!("editing USRL contracts for {}", profile.id);
    } else {
        state.message = "no selected agent to edit".to_string();
    }
}

fn apply_tui_action(
    admin: &AgentRegistryAdmin,
    state: &mut AdminTuiState,
    profiles: &[AgentProfile],
    action: TuiAction,
) -> anyhow::Result<()> {
    let Some(profile) = profiles.get(state.selected) else {
        state.message = "no selected agent".to_string();
        return Ok(());
    };
    match action {
        TuiAction::Validate => {
            let report = admin.validate_profile(profile)?;
            state.message = format!(
                "validation {}: {} errors, {} warnings",
                report.id,
                report.errors.len(),
                report.warnings.len()
            );
        }
        TuiAction::Metrics => {
            let report = load_metrics_report(&admin.data_root, &profile.id)?;
            state.message = format!(
                "metrics {}: tasks={} success={}",
                report.id,
                report.metrics.tasks_completed,
                percent_or_dash(report.task_success_rate)
            );
        }
        TuiAction::History => {
            let history = admin.load_history()?;
            let count = history
                .iter()
                .filter(|event| event.agent_id == profile.id)
                .count();
            state.message = format!("history {}: {} events", profile.id, count);
        }
        TuiAction::Activate => {
            tui_set_status(admin, &profile.id, "active")?;
            state.message = format!("status {}: active", profile.id);
        }
        TuiAction::Pause => {
            tui_set_status(admin, &profile.id, "paused")?;
            state.message = format!("status {}: paused", profile.id);
        }
        TuiAction::Deprecate => {
            tui_set_status(admin, &profile.id, "deprecated")?;
            state.message = format!("status {}: deprecated", profile.id);
        }
        TuiAction::Archive => {
            tui_set_status(admin, &profile.id, "archived")?;
            state.message = format!("status {}: archived", profile.id);
        }
        TuiAction::EditProvider => begin_provider_input(state, profiles),
        TuiAction::ClearProvider => {
            tui_set_provider(admin, &profile.id, None)?;
            state.message = format!("provider/model cleared for {}", profile.id);
        }
        TuiAction::EditModel => begin_model_input(state, profiles),
        TuiAction::ClearModel => {
            tui_set_model(admin, &profile.id, None)?;
            state.message = format!("model cleared for {}", profile.id);
        }
        TuiAction::EditTools => begin_tools_input(state, profiles),
        TuiAction::ClearTools => {
            tui_set_tools(admin, &profile.id, Vec::new())?;
            state.message = format!("tools cleared for {}", profile.id);
        }
        TuiAction::EditSkills => begin_skills_input(state, profiles),
        TuiAction::ClearSkills => {
            tui_set_skills(admin, &profile.id, Vec::new())?;
            state.message = format!("skills cleared for {}", profile.id);
        }
        TuiAction::EditMcp => begin_mcp_input(state, profiles),
        TuiAction::ClearMcp => {
            tui_set_mcp_servers(admin, &profile.id, Vec::new())?;
            state.message = format!("MCP servers cleared for {}", profile.id);
        }
        TuiAction::EditUsrl => begin_usrl_input(state, profiles),
        TuiAction::ClearUsrl => {
            tui_set_usrl_contracts(admin, &profile.id, Vec::new())?;
            state.message = format!("USRL contracts cleared for {}", profile.id);
        }
        TuiAction::EditPrimaryScope => begin_primary_scope_input(state, profiles),
        TuiAction::EditSecondaryScopes => begin_secondary_scopes_input(state, profiles),
        TuiAction::EditWorkspaceScope => begin_workspace_scope_input(state, profiles),
        TuiAction::EditFileScope => begin_file_scope_input(state, profiles),
        TuiAction::ClearScopeMetadata => {
            tui_clear_scope_metadata(admin, &profile.id)?;
            state.message = format!("scope metadata cleared for {}", profile.id);
        }
        TuiAction::EditMemoryPolicy => begin_memory_policy_input(state, profiles),
        TuiAction::EditBudgetMaxSteps => begin_budget_max_steps_input(state, profiles),
        TuiAction::EditBudgetMaxToolCalls => begin_budget_max_tool_calls_input(state, profiles),
        TuiAction::EditBudgetMaxReadBytes => begin_budget_max_read_bytes_input(state, profiles),
        TuiAction::EditBudgetMaxOutputBytes => begin_budget_max_output_bytes_input(state, profiles),
        TuiAction::EditBudgetAllowedTools => begin_budget_allowed_tools_input(state, profiles),
        TuiAction::EditBudgetNotes => begin_budget_notes_input(state, profiles),
        TuiAction::ClearBudget => {
            tui_clear_budget(admin, &profile.id)?;
            state.message = format!("default work budget cleared for {}", profile.id);
        }
        TuiAction::EditTags => begin_tags_input(state, profiles),
        TuiAction::ClearTags => {
            tui_set_tags(admin, &profile.id, Vec::new())?;
            state.message = format!("tags cleared for {}", profile.id);
        }
    }
    Ok(())
}

fn tui_set_status(admin: &AgentRegistryAdmin, id: &str, status: &str) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    if status == "active" {
        let report = admin.validate_profile(&profile)?;
        if !report.errors.is_empty() {
            bail!(
                "refusing to activate {}: validation has {} error(s)",
                profile.id,
                report.errors.len()
            );
        }
    }
    profile
        .metadata
        .insert("status".to_string(), Value::String(status.to_string()));
    admin.save_touched_quiet(profile, "tui-status")?;
    Ok(())
}

fn tui_set_metadata_string(
    admin: &AgentRegistryAdmin,
    id: &str,
    key: &str,
    value: Option<String>,
    action: &str,
) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    match value {
        Some(value) => {
            profile
                .metadata
                .insert(key.to_string(), Value::String(value));
        }
        None => {
            profile.metadata.remove(key);
        }
    }
    admin.save_touched_quiet(profile, action)?;
    Ok(())
}

fn tui_set_metadata_list(
    admin: &AgentRegistryAdmin,
    id: &str,
    key: &str,
    values: Vec<String>,
    action: &str,
) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    if values.is_empty() {
        profile.metadata.remove(key);
    } else {
        profile.metadata.insert(key.to_string(), json!(values));
    }
    admin.save_touched_quiet(profile, action)?;
    Ok(())
}

fn tui_clear_scope_metadata(admin: &AgentRegistryAdmin, id: &str) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    profile.metadata.remove("primary_scope");
    profile.metadata.remove("secondary_scopes");
    profile.metadata.remove("workspace_scope");
    profile.metadata.remove("file_scope_hints");
    admin.save_touched_quiet(profile, "tui-scope")?;
    Ok(())
}

fn tui_set_memory_policy(
    admin: &AgentRegistryAdmin,
    id: &str,
    policy: String,
) -> anyhow::Result<()> {
    let trimmed = policy.trim();
    if trimmed.is_empty() {
        bail!("memory policy must not be empty");
    }
    let mut profile = admin.store.load(id)?;
    profile.memory_policy = trimmed.to_string();
    admin.save_touched_quiet(profile, "tui-memory-policy")?;
    Ok(())
}

fn parse_optional_u64_input(input: &str, label: &str) -> anyhow::Result<Option<u64>> {
    let Some(value) = none_marker(input.to_string()) else {
        return Ok(None);
    };
    value
        .parse::<u64>()
        .map(Some)
        .with_context(|| format!("budget {label} must be a non-negative integer, '-' or clear"))
}

fn tui_update_budget_metadata<F>(
    admin: &AgentRegistryAdmin,
    id: &str,
    update: F,
) -> anyhow::Result<()>
where
    F: FnOnce(&mut serde_json::Map<String, Value>) -> anyhow::Result<()>,
{
    let mut profile = admin.store.load(id)?;
    let mut budget = profile
        .metadata
        .get("default_work_budget")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !budget.is_object() {
        budget = json!({});
    }
    {
        let map = budget.as_object_mut().expect("object ensured");
        update(map)?;
    }
    if budget.as_object().map(|map| map.is_empty()).unwrap_or(true) {
        profile.metadata.remove("default_work_budget");
    } else {
        profile
            .metadata
            .insert("default_work_budget".to_string(), budget);
    }
    admin.save_touched_quiet(profile, "tui-budget")?;
    Ok(())
}

fn tui_set_budget_u64(
    admin: &AgentRegistryAdmin,
    id: &str,
    key: &str,
    value: Option<u64>,
) -> anyhow::Result<()> {
    tui_update_budget_metadata(admin, id, |budget| {
        match value {
            Some(value) => {
                budget.insert(key.to_string(), json!(value));
            }
            None => {
                budget.remove(key);
            }
        }
        Ok(())
    })
}

fn tui_set_budget_allowed_tools(
    admin: &AgentRegistryAdmin,
    id: &str,
    tools: Vec<String>,
) -> anyhow::Result<()> {
    validate_tool_allow_list(&tools)?;
    tui_update_budget_metadata(admin, id, |budget| {
        if tools.is_empty() {
            budget.remove("allowed_tools");
        } else {
            budget.insert("allowed_tools".to_string(), json!(tools));
        }
        Ok(())
    })
}

fn tui_set_budget_notes(
    admin: &AgentRegistryAdmin,
    id: &str,
    notes: Option<String>,
) -> anyhow::Result<()> {
    tui_update_budget_metadata(admin, id, |budget| {
        match notes {
            Some(notes) => {
                budget.insert("notes".to_string(), json!(notes));
            }
            None => {
                budget.remove("notes");
            }
        }
        Ok(())
    })
}

fn tui_clear_budget(admin: &AgentRegistryAdmin, id: &str) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    profile.metadata.remove("default_work_budget");
    admin.save_touched_quiet(profile, "tui-budget")?;
    Ok(())
}

fn tui_set_provider(
    admin: &AgentRegistryAdmin,
    id: &str,
    provider: Option<String>,
) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    match provider {
        Some(provider) => {
            let providers = ProviderRegistry::default_catalog()?;
            if providers.get(&provider).is_none() {
                bail!("unknown provider: {provider}");
            }
            if let Some(model) = &profile.current_model {
                let models = ModelRegistry::default_catalog()?;
                if let Some(model_info) = models.get(model)
                    && !models.is_model_allowed_for_provider(model_info, &provider)
                {
                    bail!(
                        "model {model} is not allowed for provider {provider}; clear or change model first"
                    );
                }
            }
            profile.current_provider = Some(provider);
        }
        None => {
            profile.current_provider = None;
            profile.current_model = None;
        }
    }
    admin.save_touched_quiet(profile, "tui-provider")?;
    Ok(())
}

fn tui_set_model(
    admin: &AgentRegistryAdmin,
    id: &str,
    model: Option<String>,
) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    if let Some(model) = model {
        let models = ModelRegistry::default_catalog()?;
        let model_info = models
            .get(&model)
            .with_context(|| format!("unknown model: {model}"))?;
        if let Some(provider) = &profile.current_provider
            && !models.is_model_allowed_for_provider(model_info, provider)
        {
            bail!("model {model} is not allowed for provider {provider}");
        }
        profile.current_model = Some(model);
    } else {
        profile.current_model = None;
    }
    admin.save_touched_quiet(profile, "tui-model")?;
    Ok(())
}

fn tui_set_tools(admin: &AgentRegistryAdmin, id: &str, tools: Vec<String>) -> anyhow::Result<()> {
    validate_tool_allow_list(&tools)?;
    let mut profile = admin.store.load(id)?;
    profile.enabled_tools = tools;
    admin.save_touched_quiet(profile, "tui-tools")?;
    Ok(())
}

fn tui_set_skills(admin: &AgentRegistryAdmin, id: &str, skills: Vec<String>) -> anyhow::Result<()> {
    validate_skill_allow_list(&admin.workspace, &admin.data_root, &skills)?;
    let mut profile = admin.store.load(id)?;
    profile.enabled_skills = skills;
    admin.save_touched_quiet(profile, "tui-skills")?;
    Ok(())
}

fn tui_set_mcp_servers(
    admin: &AgentRegistryAdmin,
    id: &str,
    servers: Vec<String>,
) -> anyhow::Result<()> {
    validate_mcp_server_allow_list(&admin.data_root, &servers)?;
    let mut profile = admin.store.load(id)?;
    profile.enabled_mcp_servers = servers;
    admin.save_touched_quiet(profile, "tui-mcp")?;
    Ok(())
}

fn tui_set_usrl_contracts(
    admin: &AgentRegistryAdmin,
    id: &str,
    contracts: Vec<String>,
) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    profile.usrl_contracts = contracts;
    admin.save_touched_quiet(profile, "tui-usrl")?;
    Ok(())
}

fn resolve_usrl_contract_refs_for_admin(
    admin: &AgentRegistryAdmin,
    values: Vec<String>,
) -> anyhow::Result<Vec<String>> {
    let skills = load_skill_definitions(&admin.workspace, &admin.data_root)?;
    let mut resolved = Vec::new();
    for value in values {
        if let Some(skill) = skills.iter().find(|skill| {
            skill.name == value
                && (skill.kind == "usrl_contract"
                    || skill.metadata.get("format").and_then(Value::as_str) == Some("usrl"))
        }) {
            let contracts = skill
                .metadata
                .get("usrl_contracts")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if contracts.is_empty() {
                append_unique(&mut resolved, vec![value]);
            } else {
                append_unique(&mut resolved, contracts);
            }
        } else {
            append_unique(&mut resolved, vec![value]);
        }
    }
    Ok(resolved)
}

fn tui_set_tags(admin: &AgentRegistryAdmin, id: &str, tags: Vec<String>) -> anyhow::Result<()> {
    let mut profile = admin.store.load(id)?;
    if tags.is_empty() {
        profile.metadata.remove("tags");
    } else {
        profile.metadata.insert("tags".to_string(), json!(tags));
    }
    admin.save_touched_quiet(profile, "tui-tags")?;
    Ok(())
}

fn draw_admin_tui(
    frame: &mut ratatui::Frame<'_>,
    profiles: &[AgentProfile],
    selected_id: Option<&str>,
    state: &AdminTuiState,
) {
    let area = frame.area();
    let outer = Block::default()
        .title(Line::from(Span::styled(
            " vegvisir-agent-admin ",
            Style::default()
                .fg(TUI_CYAN)
                .bg(TUI_BG)
                .add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .style(Style::default().fg(TUI_FG).bg(TUI_BG))
        .border_style(Style::default().fg(TUI_BORDER).bg(TUI_BG));
    frame.render_widget(outer, area);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(inner);

    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            " Registry ",
            Style::default()
                .fg(TUI_FG)
                .bg(TUI_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" agents=", Style::default().fg(TUI_DIM).bg(TUI_BG)),
        Span::styled(
            profiles.len().to_string(),
            Style::default().fg(TUI_CYAN).bg(TUI_BG),
        ),
        Span::styled(" selected=", Style::default().fg(TUI_DIM).bg(TUI_BG)),
        Span::styled(
            selected_id.unwrap_or("-"),
            Style::default()
                .fg(TUI_GREEN)
                .bg(TUI_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" filter=", Style::default().fg(TUI_DIM).bg(TUI_BG)),
        Span::styled(
            if state.filter.trim().is_empty() {
                "-"
            } else {
                state.filter.as_str()
            },
            Style::default().fg(TUI_AMBER).bg(TUI_BG),
        ),
    ])])
    .style(Style::default().fg(TUI_FG).bg(TUI_BG))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .style(Style::default().fg(TUI_FG).bg(TUI_BG))
            .border_style(Style::default().fg(TUI_BORDER).bg(TUI_BG)),
    );
    frame.render_widget(header, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
        .split(chunks[1]);

    let items: Vec<ListItem> = profiles
        .iter()
        .enumerate()
        .map(|(idx, profile)| {
            let selected = Some(profile.id.as_str()) == selected_id;
            let marker = if selected { "❯" } else { " " };
            let status = profile
                .metadata
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("draft");
            let base = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(TUI_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TUI_FG).bg(TUI_PANEL)
            };
            let dim = if selected {
                Style::default().fg(Color::Black).bg(TUI_CYAN)
            } else {
                Style::default().fg(TUI_DIM).bg(TUI_PANEL)
            };
            let id_style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(TUI_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(TUI_FG)
                    .bg(TUI_PANEL)
                    .add_modifier(Modifier::BOLD)
            };
            let status_style = if selected {
                Style::default().fg(Color::Black).bg(TUI_CYAN)
            } else {
                Style::default().fg(status_color(status)).bg(TUI_PANEL)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} {idx:>3} "), dim),
                Span::styled(profile.id.clone(), id_style),
                Span::styled("  [", dim),
                Span::styled(status.to_string(), status_style),
                Span::styled("]", dim),
            ]))
            .style(base)
        })
        .collect();
    let list = List::new(items)
        .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
        .block(admin_tui_block("Agents", TUI_BORDER));
    frame.render_widget(list, body[0]);

    let detail = if let Some(profile) = profiles
        .iter()
        .find(|profile| Some(profile.id.as_str()) == selected_id)
    {
        Paragraph::new(vec![
            admin_tui_kv_line("id", &profile.id, TUI_CYAN),
            admin_tui_kv_line("name", &profile.display_name, TUI_FG),
            admin_tui_kv_line("mode", &profile.mode, TUI_GREEN),
            admin_tui_kv_line(
                "status",
                profile
                    .metadata
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("draft"),
                status_color(
                    profile
                        .metadata
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("draft"),
                ),
            ),
            admin_tui_kv_line(
                "provider",
                profile.current_provider.as_deref().unwrap_or("-"),
                TUI_AMBER,
            ),
            admin_tui_kv_line(
                "model",
                profile.current_model.as_deref().unwrap_or("-"),
                TUI_AMBER,
            ),
            admin_tui_kv_line(
                "primary scope",
                profile
                    .metadata
                    .get("primary_scope")
                    .and_then(Value::as_str)
                    .unwrap_or("-"),
                TUI_FG,
            ),
            admin_tui_kv_line(
                "secondary scopes",
                &metadata_list_or_dash(profile, "secondary_scopes"),
                TUI_FG,
            ),
            admin_tui_kv_line(
                "workspace scope",
                profile
                    .metadata
                    .get("workspace_scope")
                    .and_then(Value::as_str)
                    .unwrap_or("-"),
                TUI_FG,
            ),
            admin_tui_kv_line(
                "file-scope hints",
                &metadata_list_or_dash(profile, "file_scope_hints"),
                TUI_FG,
            ),
            admin_tui_kv_line("memory policy", &profile.memory_policy, TUI_GREEN),
            admin_tui_kv_line("budget", &budget_summary(profile), TUI_AMBER),
            admin_tui_kv_line(
                "tags",
                &profile
                    .metadata
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "-".to_string()),
                TUI_FG,
            ),
            admin_tui_kv_line("tools", &list_or_dash(&profile.enabled_tools), TUI_CYAN),
            admin_tui_kv_line("skills", &list_or_dash(&profile.enabled_skills), TUI_GREEN),
            admin_tui_kv_line("MCP", &list_or_dash(&profile.enabled_mcp_servers), TUI_CYAN),
            admin_tui_kv_line("USRL", &list_or_dash(&profile.usrl_contracts), TUI_AMBER),
            Line::from(""),
            Line::from(vec![Span::styled(
                "System prompt",
                Style::default()
                    .fg(TUI_FG)
                    .bg(TUI_PANEL)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(Span::styled(
                profile
                    .system_prompt
                    .lines()
                    .take(10)
                    .collect::<Vec<_>>()
                    .join("\n"),
                Style::default().fg(TUI_FG).bg(TUI_PANEL),
            )),
        ])
        .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
        .block(admin_tui_block("Selected agent", TUI_BORDER))
        .wrap(Wrap { trim: false })
    } else {
        Paragraph::new(Span::styled(
            "No agents found.",
            Style::default().fg(TUI_DIM).bg(TUI_PANEL),
        ))
        .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
        .block(admin_tui_block("Selected agent", TUI_BORDER))
    };
    frame.render_widget(detail, body[1]);

    if state.show_help {
        let help_area = centered_rect(82, 70, area);
        frame.render_widget(Clear, help_area);
        frame.render_widget(
            Block::default()
                .title(Line::from(Span::styled(
                    " Help ",
                    Style::default()
                        .fg(TUI_FG)
                        .bg(TUI_PANEL)
                        .add_modifier(Modifier::BOLD),
                )))
                .borders(Borders::ALL)
                .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
                .border_style(Style::default().fg(TUI_CYAN).bg(TUI_PANEL)),
            help_area,
        );
        let inner_help = Rect {
            x: help_area.x + 1,
            y: help_area.y + 1,
            width: help_area.width.saturating_sub(2),
            height: help_area.height.saturating_sub(2),
        };
        frame.render_widget(
            Paragraph::new(tui_help_text())
                .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
                .wrap(Wrap { trim: false }),
            inner_help,
        );
    }

    match state.mode {
        AdminTuiMode::ActionMenu => render_action_menu(frame, area, state),
        AdminTuiMode::TagsInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit tags ",
            "tags",
            "Comma-separated tags. Enter saves, Esc cancels.",
        ),
        AdminTuiMode::PrimaryScopeInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit primary scope ",
            "primary scope",
            "Primary scope label. Empty, '-' or 'clear' removes it.",
        ),
        AdminTuiMode::SecondaryScopesInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit secondary scopes ",
            "secondary scopes",
            "Comma-separated secondary scope labels. Empty input clears the list.",
        ),
        AdminTuiMode::WorkspaceScopeInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit workspace scope ",
            "workspace scope",
            "Workspace/repository scope label. Empty, '-' or 'clear' removes it.",
        ),
        AdminTuiMode::FileScopeInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit file-scope hints ",
            "file-scope hints",
            "Comma-separated workspace-relative file-scope hints. Empty input clears the list.",
        ),
        AdminTuiMode::MemoryPolicyInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit memory policy ",
            "memory policy",
            "Memory policy label. Empty, '-' or 'clear' resets to agent-scoped.",
        ),
        AdminTuiMode::BudgetMaxStepsInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit budget max steps ",
            "max steps",
            "Default max_steps for subagents. Empty, '-' or 'clear' removes it.",
        ),
        AdminTuiMode::BudgetMaxToolCallsInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit budget max tool calls ",
            "max tool calls",
            "Default max_tool_calls for subagents. Empty, '-' or 'clear' removes it.",
        ),
        AdminTuiMode::BudgetMaxReadBytesInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit budget max read bytes ",
            "max read bytes",
            "Default max_read_bytes for subagents. Empty, '-' or 'clear' removes it.",
        ),
        AdminTuiMode::BudgetMaxOutputBytesInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit budget max output bytes ",
            "max output bytes",
            "Default max_output_bytes for subagents. Empty, '-' or 'clear' removes it.",
        ),
        AdminTuiMode::BudgetAllowedToolsInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit budget allowed tools ",
            "allowed tools",
            "Comma-separated default budget allowed_tools. Empty input clears the list.",
        ),
        AdminTuiMode::BudgetNotesInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit budget notes ",
            "notes",
            "Default work-budget notes. Empty, '-' or 'clear' removes them.",
        ),
        AdminTuiMode::ProviderInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit provider ",
            "provider",
            "Provider id, '-' or 'clear' to inherit. Enter saves, Esc cancels.",
        ),
        AdminTuiMode::ModelInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit model ",
            "model",
            "Model id, '-' or 'clear' to inherit. Compatibility is validated on save.",
        ),
        AdminTuiMode::ToolsInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit tool allow-list ",
            "tools",
            "Comma-separated tool names. Use '*' alone only when intentionally unrestricted.",
        ),
        AdminTuiMode::SkillsInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit enabled skills ",
            "skills",
            "Comma-separated skill names. Enter saves after workspace catalog validation.",
        ),
        AdminTuiMode::McpInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit allowed MCP servers ",
            "mcp",
            "Comma-separated MCP server ids. Enter saves after data-root mcp.json validation.",
        ),
        AdminTuiMode::UsrlInput => render_text_input(
            frame,
            area,
            state,
            selected_id,
            " Edit bound USRL contracts ",
            "usrl",
            "Comma-separated contract refs. Known USRL skill refs expand to contract ids.",
        ),
        AdminTuiMode::Browse | AdminTuiMode::Search => {}
    }

    let footer = if state.mode == AdminTuiMode::Search {
        Paragraph::new(Line::from(vec![
            Span::styled("Search: ", Style::default().fg(TUI_CYAN).bg(TUI_PANEL)),
            Span::styled(&state.filter, Style::default().fg(TUI_FG).bg(TUI_PANEL)),
        ]))
        .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
        .block(admin_tui_block("Search", TUI_CYAN))
    } else {
        Paragraph::new(Span::styled(
            state.message.clone(),
            Style::default().fg(TUI_DIM).bg(TUI_PANEL),
        ))
        .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
        .block(admin_tui_block("Status", TUI_BORDER))
    };
    frame.render_widget(footer, chunks[2]);
}

fn render_action_menu(frame: &mut ratatui::Frame<'_>, area: Rect, state: &AdminTuiState) {
    let menu_area = centered_rect(58, 58, area);
    frame.render_widget(Clear, menu_area);
    frame.render_widget(
        Block::default()
            .title(Line::from(Span::styled(
                " Actions ",
                Style::default()
                    .fg(TUI_FG)
                    .bg(TUI_PANEL)
                    .add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
            .border_style(Style::default().fg(TUI_CYAN).bg(TUI_PANEL)),
        menu_area,
    );
    let inner = Rect {
        x: menu_area.x + 1,
        y: menu_area.y + 1,
        width: menu_area.width.saturating_sub(2),
        height: menu_area.height.saturating_sub(2),
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(inner);
    let visible_rows = usize::from(chunks[0].height.saturating_sub(2)).max(1);
    let mut start = state.action_selected.saturating_sub(visible_rows / 2);
    if start + visible_rows > TUI_ACTIONS.len() {
        start = TUI_ACTIONS.len().saturating_sub(visible_rows);
    }
    let items = TUI_ACTIONS
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
        .map(|(idx, action)| {
            let selected = idx == state.action_selected;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(TUI_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TUI_FG).bg(TUI_PANEL)
            };
            ListItem::new(Line::from(vec![
                Span::styled(if selected { "❯ " } else { "  " }, style),
                Span::styled(format!("{:>2}/{} ", idx + 1, TUI_ACTIONS.len()), style),
                Span::styled(action.label(), style),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items)
            .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
            .block(admin_tui_block("Choose", TUI_BORDER)),
        chunks[0],
    );
    let selected = TUI_ACTIONS
        .get(state.action_selected)
        .copied()
        .unwrap_or(TuiAction::Validate);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            selected.help(),
            Style::default().fg(TUI_DIM).bg(TUI_PANEL),
        )))
        .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
        .block(admin_tui_block("Enter applies, Esc cancels", TUI_BORDER))
        .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn render_text_input(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &AdminTuiState,
    selected_id: Option<&str>,
    title: &'static str,
    label: &'static str,
    hint: &'static str,
) {
    let input_area = centered_rect(72, 28, area);
    frame.render_widget(Clear, input_area);
    frame.render_widget(
        Block::default()
            .title(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(TUI_FG)
                    .bg(TUI_PANEL)
                    .add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
            .border_style(Style::default().fg(TUI_CYAN).bg(TUI_PANEL)),
        input_area,
    );
    let inner = Rect {
        x: input_area.x + 1,
        y: input_area.y + 1,
        width: input_area.width.saturating_sub(2),
        height: input_area.height.saturating_sub(2),
    };
    let text = vec![
        Line::from(vec![
            Span::styled("agent: ", Style::default().fg(TUI_DIM).bg(TUI_PANEL)),
            Span::styled(
                selected_id.unwrap_or("-"),
                Style::default().fg(TUI_GREEN).bg(TUI_PANEL),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("{label}: "),
                Style::default().fg(TUI_CYAN).bg(TUI_PANEL),
            ),
            Span::styled(&state.input, Style::default().fg(TUI_FG).bg(TUI_PANEL)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            hint,
            Style::default().fg(TUI_DIM).bg(TUI_PANEL),
        )),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn admin_tui_block(title: &'static str, border: Color) -> Block<'static> {
    Block::default()
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(TUI_FG)
                .bg(TUI_PANEL)
                .add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .style(Style::default().fg(TUI_FG).bg(TUI_PANEL))
        .border_style(Style::default().fg(border).bg(TUI_PANEL))
}

fn admin_tui_kv_line(label: &'static str, value: &str, value_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default().fg(TUI_DIM).bg(TUI_PANEL),
        ),
        Span::styled(
            value.to_string(),
            Style::default().fg(value_color).bg(TUI_PANEL),
        ),
    ])
}

fn status_color(status: &str) -> Color {
    match normalize_agent_id(status).as_str() {
        "active" | "ready" => TUI_GREEN,
        "broken" | "blocked" => TUI_RED,
        "paused" | "deprecated" => TUI_AMBER,
        "archived" => TUI_DIM,
        _ => TUI_AMBER,
    }
}

#[cfg(test)]
mod tests {
    use super::super::cli::{Cli, Command};
    use super::*;
    use crate::core::McpConfigStore;
    use clap::Parser;
    use tempfile::tempdir;

    fn write_mcp_config(admin: &AgentRegistryAdmin, ids: &[&str]) -> anyhow::Result<()> {
        let servers = ids
            .iter()
            .map(|id| crate::core::McpServerConfig {
                id: id.to_string(),
                display_name: id.to_string(),
                transport: crate::core::McpTransport::Stdio,
                command: None,
                args: Vec::new(),
                working_dir: None,
                url: None,
                enabled: true,
                hbse_secret_refs: Vec::new(),
                consumer: String::new(),
                purpose: String::new(),
                tools: Vec::new(),
                metadata: BTreeMap::new(),
                discovery_error: None,
            })
            .collect::<Vec<_>>();
        McpConfigStore::new(admin.data_root.join("mcp.json")).save(&servers)?;
        Ok(())
    }

    #[test]
    fn clean_list_splits_commas_dedupes_and_drops_empty_items() {
        assert_eq!(
            clean_list(vec![
                "read_file, run_tests".to_string(),
                "read_file".to_string(),
                "".to_string()
            ]),
            vec!["read_file".to_string(), "run_tests".to_string()]
        );
    }

    #[test]
    fn none_marker_clears_dash_and_clear() {
        assert_eq!(none_marker("-".to_string()), None);
        assert_eq!(none_marker("clear".to_string()), None);
        assert_eq!(
            none_marker("openai".to_string()),
            Some("openai".to_string())
        );
    }

    #[test]
    fn cli_global_workspace_and_scope_workspace_scope_do_not_collide() {
        let cli = Cli::parse_from([
            "vegvisir-agent-admin",
            "--workspace",
            "/tmp/workspace",
            "scope",
            "engineer",
            "--workspace-scope",
            "repo",
        ]);
        assert_eq!(cli.workspace, Some(PathBuf::from("/tmp/workspace")));
        match cli.command {
            Some(Command::Scope(args)) => {
                assert_eq!(args.workspace_scope.as_deref(), Some("repo"));
            }
            _ => panic!("expected scope command"),
        }
    }

    #[test]
    fn template_profile_carries_template_permissions() -> anyhow::Result<()> {
        let profile = profile_from_template("engineer", "build-engineer", Some("Build Engineer"))?;
        assert_eq!(profile.id, "build-engineer");
        assert_eq!(profile.mode, "engineer");
        assert!(profile.enabled_tools.contains(&"run_tests".to_string()));
        assert_eq!(profile.memory_policy, "agent-scoped");
        Ok(())
    }

    #[test]
    fn registry_create_set_and_doctor_flow() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let admin = AgentRegistryAdmin::new(tmp.path().join("data"), tmp.path().join("workspace"))?;
        admin.create_template(
            CreateTemplateArgs {
                mode: "tester".to_string(),
                id: "qa".to_string(),
                name: Some("QA".to_string()),
                description: None,
                force: false,
            },
            true,
        )?;
        admin.add_to_list("qa", ListField::Tools, "spawn_subagent".to_string(), true)?;
        admin.provider("qa", "openai-sso".to_string(), true)?;
        let profile = admin.store.load("qa")?;
        assert_eq!(profile.display_name, "QA");
        assert_eq!(profile.current_provider.as_deref(), Some("openai-sso"));
        assert!(
            profile
                .enabled_tools
                .contains(&"spawn_subagent".to_string())
        );
        Ok(())
    }

    #[test]
    fn validation_blocks_secret_like_prompt_material() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let admin = AgentRegistryAdmin::new(tmp.path().join("data"), tmp.path().join("workspace"))?;
        let mut profile = AgentProfile::new(
            "secret-review",
            "Secret Review",
            "Investigate this api_key=not-a-real-key placeholder.",
        )?;
        profile.description = "Prompt validation fixture".to_string();
        let report = admin.validate_profile(&profile)?;
        assert!(
            report.errors.iter().any(
                |issue| issue.field == "system_prompt" && issue.message.contains("secret-like")
            ),
            "expected secret-like system_prompt error, got {}",
            serde_json::to_string(&report.errors)?
        );
        Ok(())
    }

    #[test]
    fn import_export_round_trip_loads_profile_in_fresh_registry() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let source =
            AgentRegistryAdmin::new(tmp.path().join("source-data"), tmp.path().join("workspace"))?;
        let target =
            AgentRegistryAdmin::new(tmp.path().join("target-data"), tmp.path().join("workspace"))?;
        source.create_template(
            CreateTemplateArgs {
                mode: "tester".to_string(),
                id: "qa-export".to_string(),
                name: Some("QA Export".to_string()),
                description: Some("Round-trip fixture".to_string()),
                force: false,
            },
            true,
        )?;
        let export_path = tmp.path().join("qa-export.agent.json");
        source.export("qa-export", Some(export_path.clone()))?;
        target.import(&export_path, false, true)?;

        let loaded = target.store.load("qa-export")?;
        assert_eq!(loaded.display_name, "QA Export");
        assert_eq!(loaded.mode, "tester");
        assert_eq!(loaded.description, "Round-trip fixture");
        assert!(loaded.enabled_tools.contains(&"run_tests".to_string()));
        Ok(())
    }

    #[test]
    fn lossy_registry_listing_reports_invalid_profile_files() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let admin = AgentRegistryAdmin::new(tmp.path().join("data"), tmp.path().join("workspace"))?;
        std::fs::write(admin.store.root.join("broken.json"), "{not valid json")?;

        let (profiles, invalid_files) = admin.store.list_lossy()?;
        assert!(profiles.is_empty());
        assert!(
            invalid_files
                .iter()
                .any(|item| item.contains("broken.json")),
            "expected broken.json in invalid files, got {:?}",
            invalid_files
        );
        Ok(())
    }

    #[test]
    fn tui_help_does_not_advertise_vim_style_command_mode() {
        let help = tui_help_text();
        assert!(!help.contains("command mode"));
        assert!(!help.contains("Vim"));
        assert!(!help.contains("q           quit"));
        assert!(help.contains("F / Ctrl+F"));
        assert!(help.contains("There is no ':' command"));
    }

    #[test]
    fn validate_status_history_and_register_flow() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let admin = AgentRegistryAdmin::new(tmp.path().join("data"), tmp.path().join("workspace"))?;

        admin.register(
            RegisterArgs {
                builtins_only: true,
                skiller_only: false,
                dry_run: false,
            },
            true,
        )?;
        let engineer = admin.store.load("engineer")?;
        assert_eq!(engineer.mode, "engineer");

        let report = admin.validate_profile(&engineer)?;
        assert!(
            report.errors.is_empty(),
            "unexpected validation errors: {}",
            serde_json::to_string(&report.errors)?
        );

        admin.status("engineer", "active".to_string(), true)?;
        admin.scope(
            ScopeArgs {
                id: "engineer".to_string(),
                primary: Some("implementation".to_string()),
                secondary: vec!["rust".to_string(), "tests".to_string()],
                workspace_scope: Some("repo".to_string()),
                file_scope: vec!["vegvisir/src".to_string()],
            },
            true,
        )?;
        admin.tags(
            "engineer",
            vec!["runtime".to_string(), "coding".to_string()],
            true,
        )?;
        admin.budget(
            BudgetArgs {
                id: "engineer".to_string(),
                max_steps: Some(8),
                max_tool_calls: Some(20),
                max_read_bytes: Some(65536),
                max_output_bytes: Some(16384),
                allowed_tools: vec!["read_file".to_string(), "run_tests".to_string()],
                notes: Some("focused engineering work".to_string()),
                clear: false,
            },
            true,
        )?;

        let profile = admin.store.load("engineer")?;
        assert_eq!(
            profile.metadata.get("status").and_then(Value::as_str),
            Some("active")
        );
        assert_eq!(
            profile
                .metadata
                .get("primary_scope")
                .and_then(Value::as_str),
            Some("implementation")
        );
        assert!(profile.metadata.get("default_work_budget").is_some());
        let history = admin.load_history()?;
        assert!(history.iter().any(|event| event.agent_id == "engineer"));
        Ok(())
    }

    #[test]
    fn tui_help_advertises_action_menu_without_vim_command_mode() {
        let help = tui_help_text();
        assert!(help.contains("F2 / A"));
        assert!(help.contains("E           edit primary scope metadata"));
        assert!(help.contains("Y           edit memory policy"));
        assert!(help.contains("B           edit budget max steps"));
        assert!(help.contains("P           edit provider"));
        assert!(help.contains("O           edit model"));
        assert!(help.contains("U           edit comma-separated tool allow-list"));
        assert!(help.contains("S           edit comma-separated enabled skills"));
        assert!(help.contains("D           edit comma-separated allowed MCP servers"));
        assert!(help.contains("L           edit comma-separated bound USRL contracts"));
        assert!(help.contains("Action menu:"));
        assert!(help.contains("Scope/memory/budget/provider/model/permission/tag edit modes:"));
        assert!(!help.contains("Vim"));
        assert!(help.contains("There is no ':' command"));
    }

    #[test]
    fn tui_status_tags_provider_model_tools_and_skills_write_history_without_stdout_path()
    -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let admin = AgentRegistryAdmin::new(tmp.path().join("data"), tmp.path().join("workspace"))?;
        admin.create_template(
            CreateTemplateArgs {
                mode: "tester".to_string(),
                id: "qa".to_string(),
                name: Some("QA".to_string()),
                description: None,
                force: false,
            },
            true,
        )?;
        write_mcp_config(&admin, &["local-docs", "issue-tracker"])?;

        tui_set_status(&admin, "qa", "paused")?;
        tui_set_metadata_string(
            &admin,
            "qa",
            "primary_scope",
            Some("testing".to_string()),
            "tui-scope",
        )?;
        tui_set_metadata_list(
            &admin,
            "qa",
            "secondary_scopes",
            clean_list(vec!["rust, cli, rust".to_string()]),
            "tui-scope",
        )?;
        tui_set_metadata_string(
            &admin,
            "qa",
            "workspace_scope",
            Some("repo".to_string()),
            "tui-scope",
        )?;
        tui_set_metadata_list(
            &admin,
            "qa",
            "file_scope_hints",
            clean_list(vec!["vegvisir/src, docs".to_string()]),
            "tui-scope",
        )?;
        tui_set_memory_policy(&admin, "qa", "project-scoped".to_string())?;
        tui_set_budget_u64(&admin, "qa", "max_steps", Some(6))?;
        tui_set_budget_u64(&admin, "qa", "max_tool_calls", Some(12))?;
        tui_set_budget_u64(&admin, "qa", "max_read_bytes", Some(32768))?;
        tui_set_budget_u64(&admin, "qa", "max_output_bytes", Some(8192))?;
        tui_set_budget_allowed_tools(
            &admin,
            "qa",
            clean_list(vec!["read_file, run_tests, read_file".to_string()]),
        )?;
        tui_set_budget_notes(&admin, "qa", Some("focused qa budget".to_string()))?;
        tui_set_provider(&admin, "qa", Some("demo".to_string()))?;
        tui_set_model(&admin, "qa", Some("demo-local".to_string()))?;
        tui_set_tools(
            &admin,
            "qa",
            clean_list(vec!["read_file, run_tests, read_file".to_string()]),
        )?;
        tui_set_skills(
            &admin,
            "qa",
            clean_list(vec!["repo-orientation, code-review".to_string()]),
        )?;
        tui_set_mcp_servers(
            &admin,
            "qa",
            clean_list(vec!["local-docs, issue-tracker, local-docs".to_string()]),
        )?;
        tui_set_usrl_contracts(
            &admin,
            "qa",
            resolve_usrl_contract_refs_for_admin(
                &admin,
                clean_list(vec!["safe-dev, safe-dev".to_string()]),
            )?,
        )?;
        tui_set_tags(
            &admin,
            "qa",
            clean_list(vec!["runtime, qa, runtime".to_string()]),
        )?;

        let profile = admin.store.load("qa")?;
        assert_eq!(
            profile.metadata.get("status").and_then(Value::as_str),
            Some("paused")
        );
        assert_eq!(
            profile
                .metadata
                .get("primary_scope")
                .and_then(Value::as_str),
            Some("testing")
        );
        assert_eq!(
            metadata_string_list(&profile, "secondary_scopes"),
            vec!["rust".to_string(), "cli".to_string()]
        );
        assert_eq!(
            profile
                .metadata
                .get("workspace_scope")
                .and_then(Value::as_str),
            Some("repo")
        );
        assert_eq!(
            metadata_string_list(&profile, "file_scope_hints"),
            vec!["vegvisir/src".to_string(), "docs".to_string()]
        );
        assert_eq!(profile.memory_policy, "project-scoped");
        assert_eq!(budget_u64(&profile, "max_steps"), Some(6));
        assert_eq!(budget_u64(&profile, "max_tool_calls"), Some(12));
        assert_eq!(budget_u64(&profile, "max_read_bytes"), Some(32768));
        assert_eq!(budget_u64(&profile, "max_output_bytes"), Some(8192));
        assert_eq!(
            budget_string_list(&profile, "allowed_tools"),
            vec!["read_file".to_string(), "run_tests".to_string()]
        );
        assert_eq!(budget_string(&profile, "notes"), Some("focused qa budget"));
        assert!(budget_summary(&profile).contains("steps=6"));
        assert_eq!(profile.current_provider.as_deref(), Some("demo"));
        assert_eq!(profile.current_model.as_deref(), Some("demo-local"));
        assert_eq!(
            profile.enabled_tools,
            vec!["read_file".to_string(), "run_tests".to_string()]
        );
        assert_eq!(
            profile.enabled_skills,
            vec!["repo-orientation".to_string(), "code-review".to_string()]
        );
        assert_eq!(
            profile.enabled_mcp_servers,
            vec!["local-docs".to_string(), "issue-tracker".to_string()]
        );
        assert_eq!(profile.usrl_contracts, vec!["safe-dev".to_string()]);
        assert_eq!(
            profile_tags(&profile),
            vec!["runtime".to_string(), "qa".to_string()]
        );

        assert!(tui_set_provider(&admin, "qa", Some("openai".to_string())).is_err());
        assert!(tui_set_tools(&admin, "qa", vec!["not-a-tool".to_string()]).is_err());
        assert!(
            tui_set_tools(&admin, "qa", vec!["*".to_string(), "read_file".to_string()]).is_err()
        );
        assert!(tui_set_skills(&admin, "qa", vec!["not-a-skill".to_string()]).is_err());
        assert!(tui_set_mcp_servers(&admin, "qa", vec!["not-a-server".to_string()]).is_err());
        assert!(
            tui_set_budget_allowed_tools(&admin, "qa", vec!["not-a-tool".to_string()]).is_err()
        );
        assert!(parse_optional_u64_input("not-a-number", "max steps").is_err());
        assert_eq!(parse_optional_u64_input("clear", "max steps")?, None);
        tui_set_metadata_string(&admin, "qa", "primary_scope", None, "tui-scope")?;
        tui_clear_scope_metadata(&admin, "qa")?;
        tui_set_memory_policy(&admin, "qa", "agent-scoped".to_string())?;
        tui_set_budget_u64(&admin, "qa", "max_steps", None)?;
        tui_set_budget_allowed_tools(&admin, "qa", Vec::new())?;
        tui_set_budget_notes(&admin, "qa", None)?;
        tui_clear_budget(&admin, "qa")?;
        tui_set_provider(&admin, "qa", None)?;
        let profile = admin.store.load("qa")?;
        assert_eq!(profile.current_provider, None);
        assert_eq!(profile.current_model, None);
        tui_set_tags(&admin, "qa", Vec::new())?;
        let profile = admin.store.load("qa")?;
        assert_eq!(profile.metadata.get("primary_scope"), None);
        assert_eq!(profile.metadata.get("secondary_scopes"), None);
        assert_eq!(profile.metadata.get("tags"), None);
        assert_eq!(profile.metadata.get("default_work_budget"), None);
        assert_eq!(profile.memory_policy, "agent-scoped");

        let history = admin.load_history()?;
        assert!(history.iter().any(|event| event.action == "tui-status"));
        assert!(history.iter().any(|event| event.action == "tui-scope"));
        assert!(
            history
                .iter()
                .any(|event| event.action == "tui-memory-policy")
        );
        assert!(history.iter().any(|event| event.action == "tui-budget"));
        assert!(history.iter().any(|event| event.action == "tui-provider"));
        assert!(history.iter().any(|event| event.action == "tui-model"));
        assert!(history.iter().any(|event| event.action == "tui-tools"));
        assert!(history.iter().any(|event| event.action == "tui-skills"));
        assert!(history.iter().any(|event| event.action == "tui-mcp"));
        assert!(history.iter().any(|event| event.action == "tui-usrl"));
        assert!(history.iter().any(|event| event.action == "tui-tags"));
        Ok(())
    }
}
