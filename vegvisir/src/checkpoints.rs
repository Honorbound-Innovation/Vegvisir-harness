use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{context::ContextManager, state::RunState};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub state: RunState,
    pub context: ContextManager,
    #[serde(
        default,
        alias = "memory_root",
        skip_serializing_if = "Option::is_none"
    )]
    pub cms_root: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CheckpointStore {
    pub root: PathBuf,
}

impl CheckpointStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn path_for(&self, run_id: &str) -> PathBuf {
        self.root.join(format!("{run_id}.snapshot.json"))
    }

    pub fn save(&self, snapshot: &RunSnapshot) -> anyhow::Result<PathBuf> {
        let path = self.path_for(&snapshot.state.run_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_string_pretty(snapshot)?)?;
        Ok(path)
    }

    pub fn load(&self, run_id: &str) -> anyhow::Result<RunSnapshot> {
        Ok(serde_json::from_str(&fs::read_to_string(
            self.path_for(run_id),
        )?)?)
    }
}
