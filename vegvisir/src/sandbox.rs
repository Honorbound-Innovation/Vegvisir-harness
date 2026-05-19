use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct WorkspaceSandbox {
    pub root: PathBuf,
    pub allow_network: bool,
    pub bypass: bool,
}

impl WorkspaceSandbox {
    pub fn new(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            root: root.as_ref().canonicalize()?,
            allow_network: false,
            bypass: false,
        })
    }

    pub fn new_unrestricted(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            root: root.as_ref().canonicalize()?,
            allow_network: true,
            bypass: true,
        })
    }

    pub fn resolve(&self, path: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        let path = path.as_ref();
        let raw = path.to_string_lossy().to_string();
        if !self.bypass && (raw == "~" || raw.starts_with("~/")) {
            anyhow::bail!(
                "Home-relative paths are not supported in workspace tools; use a workspace-relative path."
            );
        }
        let candidate = if path.is_absolute() {
            path.components().collect::<PathBuf>()
        } else {
            self.root.join(path).components().collect::<PathBuf>()
        };
        let resolved = if candidate.exists() {
            candidate.canonicalize()?
        } else {
            let parent = candidate.parent().unwrap_or(&self.root);
            parent
                .canonicalize()?
                .join(candidate.file_name().unwrap_or_default())
        };
        if !self.bypass && resolved != self.root && !resolved.starts_with(&self.root) {
            anyhow::bail!("Path escapes workspace: {raw}");
        }
        Ok(resolved)
    }

    pub fn read_text(&self, path: impl AsRef<Path>) -> anyhow::Result<String> {
        Ok(fs::read_to_string(self.resolve(path)?)?)
    }

    pub fn write_text(&self, path: impl AsRef<Path>, content: &str) -> anyhow::Result<PathBuf> {
        let target = self.resolve(path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, content)?;
        Ok(target)
    }
}
