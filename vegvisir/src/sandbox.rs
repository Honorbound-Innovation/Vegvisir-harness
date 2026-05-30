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
            self.resolve_missing_candidate(&candidate)?
        };
        if !self.bypass && resolved != self.root && !resolved.starts_with(&self.root) {
            anyhow::bail!("Path escapes workspace: {raw}");
        }
        Ok(resolved)
    }

    fn resolve_missing_candidate(&self, candidate: &Path) -> anyhow::Result<PathBuf> {
        let mut existing = candidate;
        let mut missing_components = Vec::new();
        while !existing.exists() {
            let Some(parent) = existing.parent() else {
                anyhow::bail!("Path has no existing ancestor: {}", candidate.display());
            };
            if let Some(name) = existing.file_name() {
                missing_components.push(name.to_os_string());
            }
            existing = parent;
        }
        let mut resolved = existing.canonicalize()?;
        for component in missing_components.into_iter().rev() {
            resolved.push(component);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_new_nested_paths_inside_workspace() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let written = sandbox.write_text("new/dir/file.txt", "ok")?;
        assert!(written.starts_with(dir.path().canonicalize()?));
        assert_eq!(
            fs::read_to_string(dir.path().join("new/dir/file.txt"))?,
            "ok"
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_paths_that_escape_workspace() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let err = sandbox
            .write_text("../escape/file.txt", "nope")
            .unwrap_err();
        assert!(err.to_string().contains("escapes workspace"));
        Ok(())
    }
}
