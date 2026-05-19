use std::{collections::BTreeMap, fs, path::Path};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub fn utc_now() -> DateTime<Utc> {
    Utc::now()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressItem {
    pub description: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunState {
    pub goal: String,
    pub run_id: String,
    pub step: usize,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub progress: Vec<ProgressItem>,
    #[serde(default)]
    pub artifacts: BTreeMap<String, String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

impl RunState {
    pub fn new(goal: impl Into<String>) -> Self {
        let now = utc_now();
        Self {
            goal: goal.into(),
            run_id: Uuid::new_v4().simple().to_string(),
            step: 0,
            status: "running".to_string(),
            created_at: now,
            updated_at: now,
            progress: Vec::new(),
            artifacts: BTreeMap::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn mark_updated(&mut self) {
        self.updated_at = utc_now();
    }

    pub fn checkpoint(&mut self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        self.mark_updated();
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }
}
