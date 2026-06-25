use crate::{MspResult, MspSchemaKind, parse_and_validate_json_schema, validate_json_schema};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MspSkillPublicationDraft {
    pub kind: String,
    pub draft_version: String,
    pub publisher: PublicationDraftPublisher,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<PublicationDraftPack>,
    pub skills: Vec<PublicationDraftSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationDraftPublisher {
    pub issuer: String,
    pub source: String,
    #[serde(default)]
    pub unsigned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationDraftPack {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationDraftSkill {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    pub name: String,
    pub version: String,
    pub category: String,
    pub summary: String,
    pub body: String,
    #[serde(default)]
    pub task_patterns: Vec<String>,
    #[serde(default)]
    pub required_checks: Vec<String>,
    #[serde(default)]
    pub source_documents: Vec<String>,
}

impl MspSkillPublicationDraft {
    pub fn from_path(path: impl AsRef<Path>) -> MspResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let value = parse_and_validate_json_schema(MspSchemaKind::PublicationDraft, &content)?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn validate(&self) -> MspResult<()> {
        let value = serde_json::to_value(self)?;
        validate_json_schema(MspSchemaKind::PublicationDraft, &value)
    }
}
