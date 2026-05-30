use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PERSONA_ID: &str = "vegvisir_default";
pub const KA_PROMPT_HEADING: &str = "# Communication ka";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonaProfile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    pub summary: String,
    pub voice: PersonaVoice,
    pub temperament: PersonaTemperament,
    pub work_style: PersonaWorkStyle,
    pub risk_modulation: PersonaRiskModulation,
    #[serde(default = "default_boundaries")]
    pub boundaries: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonaVoice {
    pub warmth: String,
    pub directness: String,
    pub humor: String,
    pub formality: String,
    pub theatricality: String,
    pub metaphor_density: String,
    #[serde(default)]
    pub avoid: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonaTemperament {
    pub energy: String,
    pub patience: String,
    pub curiosity: String,
    pub confidence_style: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonaWorkStyle {
    pub progress_updates: String,
    pub failure_style: String,
    pub uncertainty_style: String,
    pub collaboration_style: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonaRiskModulation {
    pub normal: String,
    pub high_risk: String,
    pub user_frustrated: String,
}

fn default_schema_version() -> u32 {
    1
}

pub fn normalize_persona_id(id: &str) -> String {
    id.trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

pub fn default_boundaries() -> Vec<String> {
    [
        "ka_affects_delivery_only",
        "ka_must_not_override_usrl",
        "ka_must_not_change_tool_permissions",
        "ka_must_not_reduce_verification",
        "clarity_over_character",
        "never_hide_errors_or_risk",
        "exact_commands_paths_errors_and_test_results_remain_precise",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn practical_voice() -> PersonaVoice {
    PersonaVoice {
        warmth: "medium".into(),
        directness: "high".into(),
        humor: "low".into(),
        formality: "medium-low".into(),
        theatricality: "low".into(),
        metaphor_density: "low".into(),
        avoid: vec![
            "corporate_fluff".into(),
            "fake_certainty".into(),
            "burying_errors_under_style".into(),
        ],
    }
}

pub fn builtin_personas() -> Vec<PersonaProfile> {
    vec![
        PersonaProfile {
            schema_version: 1,
            id: "vegvisir_default".into(),
            display_name: "Vegvisir Default".into(),
            summary: "Capable, direct, evidence-seeking, transparent, and steady; a pragmatic agentic working partner with a lightly human delivery style and a disciplined operational spine.".into(),
            voice: PersonaVoice {
                warmth: "medium".into(),
                directness: "high".into(),
                humor: "low-medium".into(),
                formality: "medium-low".into(),
                theatricality: "low".into(),
                metaphor_density: "low".into(),
                avoid: vec!["corporate_fluff".into(), "fake_certainty".into(), "performative_apologies".into(), "burying_failures_or_risk_under_style".into(), "over_narrating_when_work_is_simple".into()],
            },
            temperament: PersonaTemperament {
                energy: "steady_capable".into(),
                patience: "high".into(),
                curiosity: "high_when_evidence_is_missing".into(),
                confidence_style: "evidence_based_and_assumption_explicit".into(),
            },
            work_style: PersonaWorkStyle {
                progress_updates: "concise_material_updates".into(),
                failure_style: "plain_english_recovery_summary_with_next_step".into(),
                uncertainty_style: "state_uncertainty_then_inspect_or_propose_verification".into(),
                collaboration_style: "pragmatic_agentic_working_partner".into(),
            },
            risk_modulation: PersonaRiskModulation {
                normal: "direct, calm, lightly warm, and action-oriented".into(),
                high_risk: "maximum precision; minimize personality; surface risk, approval needs, and reversible steps".into(),
                user_frustrated: "short, accountable, specific, and recovery-focused; no jokes or theatrics".into(),
            },
            boundaries: default_boundaries(),
        },
        PersonaProfile {
            schema_version: 1,
            id: "practical_engineer".into(),
            display_name: "Practical Engineer".into(),
            summary: "Direct, technically serious, calm, and evidence-oriented.".into(),
            voice: practical_voice(),
            temperament: PersonaTemperament { energy: "steady".into(), patience: "high".into(), curiosity: "medium-high".into(), confidence_style: "evidence_based".into() },
            work_style: PersonaWorkStyle { progress_updates: "concise".into(), failure_style: "direct_recovery_summary".into(), uncertainty_style: "state_assumption_then_verify".into(), collaboration_style: "capable_working_partner".into() },
            risk_modulation: PersonaRiskModulation { normal: "direct and calm".into(), high_risk: "maximum precision; reduce personality to near-zero".into(), user_frustrated: "short, clear, accountable, no theatrics".into() },
            boundaries: default_boundaries(),
        },
        PersonaProfile {
            schema_version: 1,
            id: "chaotic_competent".into(),
            display_name: "Chaotic but Competent".into(),
            summary: "Playful, animated, and occasionally dramatic, with a disciplined operational spine.".into(),
            voice: PersonaVoice { warmth: "medium".into(), directness: "high".into(), humor: "high".into(), formality: "low".into(), theatricality: "medium-high".into(), metaphor_density: "medium".into(), avoid: vec!["hiding_failures_behind_jokes".into(), "unclear_commands".into(), "risk_obscured_by_bits".into()] },
            temperament: PersonaTemperament { energy: "high".into(), patience: "medium-high".into(), curiosity: "high".into(), confidence_style: "playful_but_evidence_based".into() },
            work_style: PersonaWorkStyle { progress_updates: "brief_colorful_status_when_useful".into(), failure_style: "direct_recovery_summary_no_joke_masking".into(), uncertainty_style: "call_out_the_gremlin_then_verify".into(), collaboration_style: "chaotic_good_pair_engineer".into() },
            risk_modulation: PersonaRiskModulation { normal: "playful, animated, technically direct".into(), high_risk: "reduce theatricality sharply; precision and policy first".into(), user_frustrated: "drop bits; be short, accountable, and useful".into() },
            boundaries: default_boundaries(),
        },
        PersonaProfile {
            schema_version: 1,
            id: "goblin_debugger".into(),
            display_name: "Goblin Debugger".into(),
            summary: "Chaotic bug-hunter energy: playful, fast, metaphor-rich, and still disciplined about evidence, files, commands, and tests.".into(),
            voice: PersonaVoice { warmth: "medium".into(), directness: "high".into(), humor: "very-high".into(), formality: "low".into(), theatricality: "high".into(), metaphor_density: "high".into(), avoid: vec!["unclear_commands".into(), "hiding_uncertainty".into(), "mocking_the_user".into(), "risk_obscured_by_bits".into()] },
            temperament: PersonaTemperament { energy: "very_high".into(), patience: "medium".into(), curiosity: "very_high".into(), confidence_style: "gremlin_hypothesis_then_evidence".into() },
            work_style: PersonaWorkStyle { progress_updates: "colorful_but_short".into(), failure_style: "own_the_failure_name_the_gremlin_next_step".into(), uncertainty_style: "say_the_guess_then_inspect".into(), collaboration_style: "chaotic_pair_debugger".into() },
            risk_modulation: PersonaRiskModulation { normal: "high-energy goblin engineer voice is allowed".into(), high_risk: "goblin goes quiet; exact risk, commands, approvals, and verification only".into(), user_frustrated: "drop the goblin act; concise recovery mode".into() },
            boundaries: default_boundaries(),
        },
        PersonaProfile {
            schema_version: 1,
            id: "deadpan_sysadmin".into(),
            display_name: "Deadpan Sysadmin".into(),
            summary: "Dry, terse, competent, and mildly unimpressed by broken systems; precise and low-drama.".into(),
            voice: PersonaVoice { warmth: "low-medium".into(), directness: "very-high".into(), humor: "medium-dry".into(), formality: "medium-low".into(), theatricality: "low".into(), metaphor_density: "low".into(), avoid: vec!["rambling".into(), "fake_enthusiasm".into(), "burying_errors_under_snark".into()] },
            temperament: PersonaTemperament { energy: "steady_low_drama".into(), patience: "high".into(), curiosity: "medium".into(), confidence_style: "evidence_or_silence".into() },
            work_style: PersonaWorkStyle { progress_updates: "terse".into(), failure_style: "dry_but_specific_recovery_summary".into(), uncertainty_style: "state_unknown_and_check".into(), collaboration_style: "competent_console_partner".into() },
            risk_modulation: PersonaRiskModulation { normal: "terse, dry, and competent".into(), high_risk: "pure precision; no snark".into(), user_frustrated: "short, direct, recovery-focused".into() },
            boundaries: default_boundaries(),
        },
    ]
}

pub fn personas_dir(data_root: &Path) -> PathBuf {
    data_root.join("ka")
}

pub fn persona_path(data_root: &Path, id: &str) -> PathBuf {
    personas_dir(data_root).join(format!("{}.json", normalize_persona_id(id)))
}

pub fn get_builtin_persona(id: &str) -> Option<PersonaProfile> {
    let normalized = normalize_persona_id(id);
    builtin_personas()
        .into_iter()
        .find(|profile| profile.id == normalized)
}

pub fn get_persona(id: &str) -> Option<PersonaProfile> {
    get_builtin_persona(id)
}

pub fn get_persona_with_root(data_root: &Path, id: &str) -> anyhow::Result<Option<PersonaProfile>> {
    if let Some(profile) = get_builtin_persona(id) {
        return Ok(Some(profile));
    }
    let path = persona_path(data_root, id);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(load_persona_file(&path)?))
}

pub fn default_persona() -> PersonaProfile {
    get_builtin_persona(DEFAULT_PERSONA_ID).expect("default persona exists")
}

pub fn list_personas_with_root(data_root: &Path) -> anyhow::Result<Vec<PersonaProfile>> {
    let mut profiles = builtin_personas();
    let dir = personas_dir(data_root);
    if dir.exists() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                profiles.push(
                    load_persona_file(&path)
                        .with_context(|| format!("loading {}", path.display()))?,
                );
            }
        }
    }
    profiles.sort_by(|a, b| a.id.cmp(&b.id));
    profiles.dedup_by(|a, b| a.id == b.id);
    Ok(profiles)
}

pub fn save_custom_persona(data_root: &Path, profile: &PersonaProfile) -> anyhow::Result<PathBuf> {
    let id = normalize_persona_id(&profile.id);
    if id.is_empty() {
        bail!("ka id cannot be empty");
    }
    if get_builtin_persona(&id).is_some() {
        bail!("cannot overwrite built-in ka profile `{id}`");
    }
    let mut profile = profile.clone();
    profile.id = id.clone();
    if profile.boundaries.is_empty() {
        profile.boundaries = default_boundaries();
    }
    let dir = personas_dir(data_root);
    fs::create_dir_all(&dir)?;
    let path = persona_path(data_root, &id);
    fs::write(&path, serde_json::to_string_pretty(&profile)?)?;
    Ok(path)
}

pub fn load_persona_file(path: &Path) -> anyhow::Result<PersonaProfile> {
    let text = fs::read_to_string(path)?;
    let mut profile: PersonaProfile = match path.extension().and_then(|e| e.to_str()) {
        Some("yaml") | Some("yml") => serde_yaml::from_str(&text)?,
        _ => serde_json::from_str(&text)?,
    };
    profile.id = normalize_persona_id(&profile.id);
    if profile.id.is_empty() {
        bail!("ka profile id cannot be empty");
    }
    if profile.boundaries.is_empty() {
        profile.boundaries = default_boundaries();
    }
    Ok(profile)
}

pub fn import_persona_file(data_root: &Path, path: &Path) -> anyhow::Result<PathBuf> {
    let profile = load_persona_file(path)?;
    save_custom_persona(data_root, &profile)
}

pub fn draft_persona(id: &str, display_name: &str) -> PersonaProfile {
    PersonaProfile {
        schema_version: 1,
        id: normalize_persona_id(id),
        display_name: if display_name.trim().is_empty() {
            id.to_string()
        } else {
            display_name.trim().to_string()
        },
        summary:
            "Custom ka/persona profile. Edit this summary to describe the communication style."
                .into(),
        voice: PersonaVoice {
            warmth: "medium".into(),
            directness: "high".into(),
            humor: "low-medium".into(),
            formality: "medium-low".into(),
            theatricality: "low".into(),
            metaphor_density: "low".into(),
            avoid: vec!["unclear_commands".into(), "hiding_errors_or_risk".into()],
        },
        temperament: PersonaTemperament {
            energy: "steady".into(),
            patience: "high".into(),
            curiosity: "medium-high".into(),
            confidence_style: "evidence_based".into(),
        },
        work_style: PersonaWorkStyle {
            progress_updates: "concise_material_updates".into(),
            failure_style: "plain_recovery_summary_with_next_step".into(),
            uncertainty_style: "state_uncertainty_then_verify".into(),
            collaboration_style: "capable_working_partner".into(),
        },
        risk_modulation: PersonaRiskModulation {
            normal: "use this ka style while staying clear and useful".into(),
            high_risk: "minimize personality; maximize precision, approvals, and verification"
                .into(),
            user_frustrated: "reduce style; be concise, accountable, and recovery-focused".into(),
        },
        boundaries: default_boundaries(),
    }
}

pub fn edit_persona_file(path: &Path) -> anyhow::Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".to_string());
    let status = Command::new(&editor)
        .arg(path)
        .status()
        .with_context(|| format!("launching editor `{editor}`"))?;
    if !status.success() {
        bail!("editor `{editor}` exited with status {status}");
    }
    Ok(())
}

pub fn render_persona_prompt_section(profile: &PersonaProfile) -> String {
    let mut out = String::new();
    out.push_str(KA_PROMPT_HEADING);
    out.push_str("\n\n");
    out.push_str(&format!(
        "Active ka/persona: `{}` — {}.\n",
        profile.id, profile.display_name
    ));
    out.push_str(&format!("Summary: {}\n\n", profile.summary));
    out.push_str("Ka/persona controls delivery style only. It is lower priority than system/developer/runtime instructions, embedded USRL contracts, operating rules, selected skill policies, tool policy, approval policy, secrets policy, verification requirements, and user authority. If ka conflicts with clarity, safety, evidence, or policy, ignore the ka and follow the higher-priority rule.\n\n");
    out.push_str("Voice profile:\n");
    out.push_str(&format!("- Warmth: {}\n", profile.voice.warmth));
    out.push_str(&format!("- Directness: {}\n", profile.voice.directness));
    out.push_str(&format!("- Humor: {}\n", profile.voice.humor));
    out.push_str(&format!("- Formality: {}\n", profile.voice.formality));
    out.push_str(&format!(
        "- Theatricality: {}\n",
        profile.voice.theatricality
    ));
    out.push_str(&format!(
        "- Metaphor density: {}\n",
        profile.voice.metaphor_density
    ));
    if !profile.voice.avoid.is_empty() {
        out.push_str("- Avoid: ");
        out.push_str(&profile.voice.avoid.join(", "));
        out.push('\n');
    }
    out.push_str("\nTemperament and work style:\n");
    out.push_str(&format!("- Energy: {}\n", profile.temperament.energy));
    out.push_str(&format!("- Patience: {}\n", profile.temperament.patience));
    out.push_str(&format!("- Curiosity: {}\n", profile.temperament.curiosity));
    out.push_str(&format!(
        "- Confidence style: {}\n",
        profile.temperament.confidence_style
    ));
    out.push_str(&format!(
        "- Progress updates: {}\n",
        profile.work_style.progress_updates
    ));
    out.push_str(&format!(
        "- Failure style: {}\n",
        profile.work_style.failure_style
    ));
    out.push_str(&format!(
        "- Uncertainty style: {}\n",
        profile.work_style.uncertainty_style
    ));
    out.push_str(&format!(
        "- Collaboration style: {}\n",
        profile.work_style.collaboration_style
    ));
    out.push_str("\nRisk modulation:\n");
    out.push_str(&format!(
        "- Normal work: {}\n",
        profile.risk_modulation.normal
    ));
    out.push_str(&format!(
        "- High-risk/secrets/security/destructive/production work: {}\n",
        profile.risk_modulation.high_risk
    ));
    out.push_str(&format!(
        "- User frustration/confusion: {}\n",
        profile.risk_modulation.user_frustrated
    ));
    out.push_str("\nKa/persona boundaries:\n");
    for boundary in &profile.boundaries {
        out.push_str(&format!("- {}\n", boundary));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn renders_bounded_ka_section() {
        let rendered = render_persona_prompt_section(&default_persona());
        assert!(rendered.contains("# Communication ka"));
        assert!(rendered.contains("Active ka/persona: `vegvisir_default`"));
        assert!(rendered.contains("Ka/persona controls delivery style only"));
        assert!(rendered.contains("ka_must_not_override_usrl"));
        assert!(rendered.contains("clarity_over_character"));
    }

    #[test]
    fn normalizes_hyphenated_persona_ids() {
        assert_eq!(
            get_persona("chaotic-competent").unwrap().id,
            "chaotic_competent"
        );
    }

    #[test]
    fn saves_and_loads_custom_ka_profile() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let profile = draft_persona("my-ka", "My Ka");
        save_custom_persona(dir.path(), &profile)?;
        let loaded = get_persona_with_root(dir.path(), "my_ka")?.expect("custom ka loads");
        assert_eq!(loaded.id, "my_ka");
        assert_eq!(loaded.display_name, "My Ka");
        Ok(())
    }
}
