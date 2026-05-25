use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::*;

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct WorkspaceIndex {
    #[serde(default)]
    pub(crate) active_sessions: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) provider_overrides: BTreeMap<String, ProviderSelection>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct ProviderSelection {
    pub(crate) provider: String,
    pub(crate) model: String,
}

impl TuiApplication {
    pub(crate) fn workspace_index_path(&self) -> PathBuf {
        workspace_index_path_for_user(&self.data_root, &self.default_user_id())
    }

    pub(crate) fn load_workspace_index(&self) -> WorkspaceIndex {
        std::fs::read_to_string(self.workspace_index_path())
            .ok()
            .and_then(|text| serde_json::from_str::<WorkspaceIndex>(&text).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save_workspace_index(&self, index: &WorkspaceIndex) -> anyhow::Result<()> {
        if let Some(parent) = self.workspace_index_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            self.workspace_index_path(),
            serde_json::to_string_pretty(index)?,
        )?;
        Ok(())
    }

    pub(crate) fn load_workspace_session_index(&self) -> BTreeMap<String, String> {
        self.load_workspace_index().active_sessions
    }

    pub(crate) fn provider_selection_for_workspace(&self, workspace: &Path) -> ProviderSelection {
        let key = workspace.display().to_string();
        if let Some(selection) = self.load_workspace_index().provider_overrides.get(&key) {
            return selection.clone();
        }
        let defaults = self.config.load().unwrap_or_default();
        ProviderSelection {
            provider: defaults
                .get("current_provider")
                .and_then(Value::as_str)
                .unwrap_or("demo")
                .to_string(),
            model: defaults
                .get("current_model")
                .and_then(Value::as_str)
                .unwrap_or("demo-local")
                .to_string(),
        }
    }

    pub(crate) fn apply_provider_selection_for_workspace(&mut self) {
        let selection = self.provider_selection_for_workspace(&self.cwd);
        if self.provider_registry.get(&selection.provider).is_some() {
            self.session.current_provider = selection.provider;
        }
        match self.models.get(&selection.model) {
            Some(model)
                if self
                    .models
                    .is_model_allowed_for_provider(model, &self.session.current_provider) =>
            {
                self.session.current_model = selection.model;
                if let Some(context_window) = model.context_window {
                    self.session.context_limit = context_window;
                }
            }
            Some(_) => {
                if let Some(default) = self
                    .models
                    .default_for_provider(&self.session.current_provider)
                {
                    self.session.current_model = default.name.clone();
                    if let Some(context_window) = default.context_window {
                        self.session.context_limit = context_window;
                    }
                }
            }
            None if !selection.model.trim().is_empty() => {
                self.session.current_model = selection.model;
            }
            None => {
                if let Some(default) = self
                    .models
                    .default_for_provider(&self.session.current_provider)
                {
                    self.session.current_model = default.name.clone();
                    if let Some(context_window) = default.context_window {
                        self.session.context_limit = context_window;
                    }
                }
            }
        }
    }

    pub(crate) fn save_workspace_provider_override(&self) -> anyhow::Result<()> {
        let mut index = self.load_workspace_index();
        index.provider_overrides.insert(
            self.cwd.display().to_string(),
            ProviderSelection {
                provider: self.session.current_provider.clone(),
                model: self.session.current_model.clone(),
            },
        );
        self.save_workspace_index(&index)
    }

    pub(crate) fn clear_workspace_provider_override(&self) -> anyhow::Result<()> {
        let mut index = self.load_workspace_index();
        index
            .provider_overrides
            .remove(&self.cwd.display().to_string());
        self.save_workspace_index(&index)
    }

    pub(crate) fn remember_workspace_session(&self, workspace: &Path, session_id: &str) {
        let mut index = self.load_workspace_index();
        index
            .active_sessions
            .insert(workspace.display().to_string(), session_id.to_string());
        let _ = self.save_workspace_index(&index);
    }
}

pub(crate) fn workspace_index_path_for_user(data_root: &Path, user_id: &str) -> PathBuf {
    if user_id == "local-user" {
        return data_root.join("workspaces.json");
    }
    data_root
        .join("users")
        .join(user_storage_slug(user_id))
        .join("workspaces.json")
}

pub(crate) fn user_storage_slug(user_id: &str) -> String {
    let slug = crate::core::normalize_agent_id(user_id);
    if slug.is_empty() {
        "local-user".to_string()
    } else {
        slug
    }
}
