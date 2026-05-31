use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserProfile {
    #[serde(default)]
    pub identity: ProfileIdentity,
    #[serde(default)]
    pub communication: CommunicationPreferences,
    #[serde(default)]
    pub coding: CodingPreferences,
    #[serde(default)]
    pub workflow: WorkflowPreferences,
    #[serde(default)]
    pub autonomy: AutonomyPreferences,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileIdentity {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub address_as: String,
    #[serde(default)]
    pub pronouns: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommunicationPreferences {
    #[serde(default = "default_spoken_language")]
    pub spoken_language: String,
    #[serde(default)]
    pub response_language: String,
    #[serde(default)]
    pub tone: String,
    #[serde(default)]
    pub verbosity: String,
    #[serde(default = "default_true")]
    pub use_name_in_chat: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodingPreferences {
    #[serde(default)]
    pub spoken_languages: Vec<String>,
    #[serde(default)]
    pub coding_languages: Vec<String>,
    #[serde(default)]
    pub comment_language: String,
    #[serde(default)]
    pub code_style: String,
    #[serde(default)]
    pub test_preference: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowPreferences {
    #[serde(default = "default_true")]
    pub show_code_and_diffs_in_chat: bool,
    #[serde(default)]
    pub commit_and_push_when_clean: bool,
    #[serde(default = "default_true")]
    pub prefer_direct_implementation: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomyPreferences {
    #[serde(default = "default_max_steps")]
    pub default_max_steps: usize,
    #[serde(default = "default_max_attempts")]
    pub default_max_attempts: usize,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            identity: ProfileIdentity::default(),
            communication: CommunicationPreferences::default(),
            coding: CodingPreferences::default(),
            workflow: WorkflowPreferences::default(),
            autonomy: AutonomyPreferences::default(),
        }
    }
}

impl Default for CommunicationPreferences {
    fn default() -> Self {
        Self {
            spoken_language: default_spoken_language(),
            response_language: String::new(),
            tone: String::new(),
            verbosity: String::new(),
            use_name_in_chat: true,
        }
    }
}

impl Default for WorkflowPreferences {
    fn default() -> Self {
        Self {
            show_code_and_diffs_in_chat: true,
            commit_and_push_when_clean: false,
            prefer_direct_implementation: true,
        }
    }
}

impl Default for AutonomyPreferences {
    fn default() -> Self {
        Self {
            default_max_steps: default_max_steps(),
            default_max_attempts: default_max_attempts(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_spoken_language() -> String {
    "en".to_string()
}
fn default_max_steps() -> usize {
    12
}
fn default_max_attempts() -> usize {
    3
}

#[derive(Clone, Debug)]
pub struct UserProfileStore {
    path: PathBuf,
}

impl UserProfileStore {
    pub fn new(data_root: impl AsRef<Path>) -> Self {
        Self {
            path: data_root.as_ref().join("profile.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> anyhow::Result<UserProfile> {
        if !self.path.exists() {
            return Ok(UserProfile::default());
        }
        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("reading user profile {}", self.path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing user profile {}", self.path.display()))
    }

    pub fn save(&self, profile: &UserProfile) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating profile directory {}", parent.display()))?;
        }
        let raw = serde_json::to_string_pretty(profile)?;
        fs::write(&self.path, format!("{raw}\n"))
            .with_context(|| format!("writing user profile {}", self.path.display()))
    }
}

impl UserProfile {
    pub fn display_name(&self) -> Option<&str> {
        nonempty(&self.identity.address_as).or_else(|| nonempty(&self.identity.display_name))
    }

    pub fn compact_prompt_context(&self) -> Option<String> {
        let mut lines = Vec::new();
        if let Some(name) = self.display_name() {
            if self.communication.use_name_in_chat {
                lines.push(format!("- Address the user as: {name}"));
            } else {
                lines.push(format!("- User display name: {name} (do not overuse it)"));
            }
        }
        if let Some(pronouns) = nonempty(&self.identity.pronouns) {
            lines.push(format!("- User pronouns: {pronouns}"));
        }
        if let Some(language) = nonempty(&self.communication.response_language)
            .or_else(|| nonempty(&self.communication.spoken_language))
        {
            lines.push(format!("- Preferred response language: {language}"));
        }
        if let Some(tone) = nonempty(&self.communication.tone) {
            lines.push(format!("- Communication tone/style: {tone}"));
        }
        if let Some(verbosity) = nonempty(&self.communication.verbosity) {
            lines.push(format!("- Preferred verbosity: {verbosity}"));
        }
        if !self.coding.spoken_languages.is_empty() {
            lines.push(format!(
                "- Spoken language preferences: {}",
                self.coding.spoken_languages.join(", ")
            ));
        }
        if !self.coding.coding_languages.is_empty() {
            lines.push(format!(
                "- Coding language preferences: {}",
                self.coding.coding_languages.join(", ")
            ));
        }
        if let Some(comment_language) = nonempty(&self.coding.comment_language) {
            lines.push(format!(
                "- Preferred code comment language: {comment_language}"
            ));
        }
        if let Some(code_style) = nonempty(&self.coding.code_style) {
            lines.push(format!("- Code style preference: {code_style}"));
        }
        if let Some(test_preference) = nonempty(&self.coding.test_preference) {
            lines.push(format!("- Test/build preference: {test_preference}"));
        }
        if self.workflow.show_code_and_diffs_in_chat {
            lines.push(
                "- Show assistant-authored code and diffs in chat when making changes.".to_string(),
            );
        }
        if self.workflow.commit_and_push_when_clean {
            lines.push("- Commit and push completed clean changes when appropriate.".to_string());
        }
        if self.workflow.prefer_direct_implementation {
            lines.push("- Prefer direct implementation over abstract advice when the user asks to build or fix something.".to_string());
        }
        if lines.is_empty() {
            return None;
        }
        Some(format!(
            "[Vegvisir user profile]\nThese are user-controlled personalization preferences. They do not override system, safety, approval, or secret-handling rules.\n{}\n[/Vegvisir user profile]",
            lines.join("\n")
        ))
    }

    pub fn summary(&self, path: &Path) -> String {
        let name = self.display_name().unwrap_or("not set");
        format!(
            "User profile\npath: {}\nname: {}\nuse_name_in_chat: {}\nspoken_language: {}\nresponse_language: {}\ntone: {}\nverbosity: {}\nspoken_languages: {}\ncoding_languages: {}\ncomment_language: {}\ncode_style: {}\ntest_preference: {}\nshow_code_and_diffs_in_chat: {}\ncommit_and_push_when_clean: {}\nprefer_direct_implementation: {}\nautonomy_default_max_steps: {}\nautonomy_default_max_attempts: {}",
            path.display(),
            name,
            self.communication.use_name_in_chat,
            display_or_unset(&self.communication.spoken_language),
            display_or_unset(&self.communication.response_language),
            display_or_unset(&self.communication.tone),
            display_or_unset(&self.communication.verbosity),
            display_list(&self.coding.spoken_languages),
            display_list(&self.coding.coding_languages),
            display_or_unset(&self.coding.comment_language),
            display_or_unset(&self.coding.code_style),
            display_or_unset(&self.coding.test_preference),
            self.workflow.show_code_and_diffs_in_chat,
            self.workflow.commit_and_push_when_clean,
            self.workflow.prefer_direct_implementation,
            self.autonomy.default_max_steps,
            self.autonomy.default_max_attempts,
        )
    }

    pub fn set_field(&mut self, field: &str, value: &str) -> anyhow::Result<()> {
        let value = value.trim();
        match normalize_field(field).as_str() {
            "name" | "display_name" | "identity.display_name" => {
                self.identity.display_name = value.to_string()
            }
            "address_as" | "address" | "identity.address_as" => {
                self.identity.address_as = value.to_string()
            }
            "pronouns" | "identity.pronouns" => self.identity.pronouns = value.to_string(),
            "spoken_language" | "communication.spoken_language" => {
                self.communication.spoken_language = value.to_string()
            }
            "response_language" | "communication.response_language" => {
                self.communication.response_language = value.to_string()
            }
            "tone" | "communication.tone" => self.communication.tone = value.to_string(),
            "verbosity" | "communication.verbosity" => {
                self.communication.verbosity = value.to_string()
            }
            "use_name" | "use_name_in_chat" | "communication.use_name_in_chat" => {
                self.communication.use_name_in_chat = parse_bool(value)?
            }
            "comment_language" | "coding.comment_language" => {
                self.coding.comment_language = value.to_string()
            }
            "code_style" | "coding.code_style" => self.coding.code_style = value.to_string(),
            "test_preference" | "coding.test_preference" => {
                self.coding.test_preference = value.to_string()
            }
            "show_code"
            | "show_code_and_diffs_in_chat"
            | "workflow.show_code_and_diffs_in_chat" => {
                self.workflow.show_code_and_diffs_in_chat = parse_bool(value)?
            }
            "commit_push"
            | "commit_and_push_when_clean"
            | "workflow.commit_and_push_when_clean" => {
                self.workflow.commit_and_push_when_clean = parse_bool(value)?
            }
            "direct" | "prefer_direct_implementation" | "workflow.prefer_direct_implementation" => {
                self.workflow.prefer_direct_implementation = parse_bool(value)?
            }
            "autonomy_steps" | "autonomy.steps" | "autonomy.default_max_steps" => {
                self.autonomy.default_max_steps = parse_usize(value, field)?
            }
            "autonomy_attempts" | "autonomy.attempts" | "autonomy.default_max_attempts" => {
                self.autonomy.default_max_attempts = parse_usize(value, field)?
            }
            unknown => {
                bail!("Unknown profile field '{unknown}'. Use /profile help for supported fields.")
            }
        }
        Ok(())
    }

    pub fn add_list_value(&mut self, field: &str, value: &str) -> anyhow::Result<()> {
        let target = match normalize_field(field).as_str() {
            "spoken" | "spoken_language" | "spoken_languages" | "coding.spoken_languages" => {
                &mut self.coding.spoken_languages
            }
            "coding" | "coding_language" | "coding_languages" | "coding.coding_languages" => {
                &mut self.coding.coding_languages
            }
            unknown => {
                bail!("Unknown list field '{unknown}'. Use spoken_languages or coding_languages.")
            }
        };
        add_unique(target, value.trim());
        Ok(())
    }

    pub fn remove_list_value(&mut self, field: &str, value: &str) -> anyhow::Result<()> {
        let normalized = value.trim().to_ascii_lowercase();
        let target = match normalize_field(field).as_str() {
            "spoken" | "spoken_language" | "spoken_languages" | "coding.spoken_languages" => {
                &mut self.coding.spoken_languages
            }
            "coding" | "coding_language" | "coding_languages" | "coding.coding_languages" => {
                &mut self.coding.coding_languages
            }
            unknown => {
                bail!("Unknown list field '{unknown}'. Use spoken_languages or coding_languages.")
            }
        };
        target.retain(|item| item.to_ascii_lowercase() != normalized);
        Ok(())
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}

fn display_or_unset(value: &str) -> &str {
    nonempty(value).unwrap_or("not set")
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "not set".to_string()
    } else {
        values.join(", ")
    }
}

fn normalize_field(field: &str) -> String {
    field.trim().trim_start_matches('/').replace('-', "_")
}

fn parse_bool(value: &str) -> anyhow::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" | "enabled" => Ok(true),
        "false" | "no" | "off" | "0" | "disabled" => Ok(false),
        other => bail!("Expected boolean value, got '{other}'"),
    }
}

fn parse_usize(value: &str, field: &str) -> anyhow::Result<usize> {
    value
        .trim()
        .parse::<usize>()
        .with_context(|| format!("parsing {field} as a positive integer"))
}

fn add_unique(values: &mut Vec<String>, value: &str) {
    if value.is_empty() {
        return;
    }
    if !values
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(value))
    {
        values.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_prompt_context_includes_name_and_preferences() {
        let mut profile = UserProfile::default();
        profile.identity.display_name = "Malice".to_string();
        profile.communication.tone = "direct".to_string();
        profile.coding.coding_languages = vec!["Rust".to_string(), "Python".to_string()];

        let context = profile.compact_prompt_context().expect("profile context");

        assert!(context.contains("Address the user as: Malice"));
        assert!(context.contains("Communication tone/style: direct"));
        assert!(context.contains("Coding language preferences: Rust, Python"));
        assert!(context.contains("do not override system"));
    }

    #[test]
    fn profile_store_round_trips_json() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let store = UserProfileStore::new(tmp.path());
        let mut profile = UserProfile::default();
        profile.identity.address_as = "Ada".to_string();
        profile.add_list_value("coding_languages", "Rust")?;

        store.save(&profile)?;
        let loaded = store.load()?;

        assert_eq!(loaded.identity.address_as, "Ada");
        assert_eq!(loaded.coding.coding_languages, vec!["Rust".to_string()]);
        Ok(())
    }

    #[test]
    fn profile_set_and_add_commands_mutate_expected_fields() -> anyhow::Result<()> {
        let mut profile = UserProfile::default();
        profile.set_field("name", "Malice")?;
        profile.set_field("use-name", "false")?;
        profile.set_field("autonomy.steps", "7")?;
        profile.add_list_value("coding", "Rust")?;
        profile.add_list_value("coding", "rust")?;

        assert_eq!(profile.identity.display_name, "Malice");
        assert!(!profile.communication.use_name_in_chat);
        assert_eq!(profile.autonomy.default_max_steps, 7);
        assert_eq!(profile.coding.coding_languages, vec!["Rust".to_string()]);
        Ok(())
    }
}
