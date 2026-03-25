//! Codebase snapshot management.
//!
//! Creates frozen copies of the Python source tree keyed by `codebase_hash`.
//! Workers execute against these snapshots instead of the live codebase,
//! ensuring consistency across all materializations in a batch.
//!
//! The `.venv` is symlinked (not copied) since `uv.lock` is already part of
//! the `codebase_hash` — if dependencies change, we get a new snapshot.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use tracing::info;

/// Directories to skip when copying the project source tree.
const SKIP_DIRS: &[&str] = &[
    ".venv",
    "__pycache__",
    ".git",
    ".barca",
    ".barcafiles",
    "build",
    "dist",
    "node_modules",
    "target",
    "tmp",
];

pub struct SnapshotManager {
    snapshots_dir: PathBuf,
}

impl SnapshotManager {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            snapshots_dir: repo_root.join(".barca").join("snapshots"),
        }
    }

    /// Returns the path to an existing snapshot, or `None`.
    pub fn snapshot_path(&self, codebase_hash: &str) -> PathBuf {
        self.snapshots_dir.join(codebase_hash)
    }

    /// Returns true if a snapshot for this hash already exists.
    pub fn snapshot_exists(&self, codebase_hash: &str) -> bool {
        self.snapshot_path(codebase_hash).is_dir()
    }

    /// Create a snapshot of the Python codebase if one doesn't already exist.
    ///
    /// Copies all `.py` files (preserving directory structure), plus
    /// `pyproject.toml` and `uv.lock` if present. Symlinks `.venv`.
    pub fn ensure_snapshot(&self, repo_root: &Path, codebase_hash: &str) -> anyhow::Result<PathBuf> {
        let dest = self.snapshot_path(codebase_hash);
        if dest.is_dir() {
            return Ok(dest);
        }

        info!(codebase_hash, "creating codebase snapshot");
        fs::create_dir_all(&dest).context("failed to create snapshot directory")?;

        // Copy Python source files
        copy_py_tree(repo_root, repo_root, &dest)?;

        // Copy key config files
        for name in &["pyproject.toml", "uv.lock", "setup.py", "setup.cfg"] {
            let src = repo_root.join(name);
            if src.is_file() {
                fs::copy(&src, dest.join(name)).with_context(|| format!("failed to copy {name}"))?;
            }
        }

        // Symlink .venv so the snapshot can use the same installed packages
        let venv_src = repo_root.join(".venv");
        let venv_dest = dest.join(".venv");
        if venv_src.is_dir() && !venv_dest.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&venv_src, &venv_dest).context("failed to symlink .venv")?;
            #[cfg(not(unix))]
            fs::copy(&venv_src, &venv_dest).context("failed to copy .venv")?;
        }

        info!(codebase_hash, path = %dest.display(), "snapshot created");
        Ok(dest)
    }

    /// Remove all snapshots except those in the `keep` set.
    pub fn cleanup(&self, keep: &[&str]) -> anyhow::Result<()> {
        if !self.snapshots_dir.is_dir() {
            return Ok(());
        }
        let entries = fs::read_dir(&self.snapshots_dir).context("failed to read snapshots dir")?;
        for entry in entries.flatten() {
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            if !keep.contains(&name.as_str()) {
                let path = entry.path();
                info!(snapshot = %name, "removing old snapshot");
                if path.is_dir() {
                    // Remove symlinks first to avoid following them
                    let venv_link = path.join(".venv");
                    if venv_link.is_symlink() {
                        fs::remove_file(&venv_link).ok();
                    }
                    fs::remove_dir_all(&path).ok();
                }
            }
        }
        Ok(())
    }
}

/// Recursively copy `.py` files from `src_dir` to `dest_root`, preserving
/// directory structure relative to `root`.
fn copy_py_tree(root: &Path, src_dir: &Path, dest_root: &Path) -> anyhow::Result<()> {
    let entries = match fs::read_dir(src_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        if path.is_dir() {
            copy_py_tree(root, &path, dest_root)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("py") {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let dest_file = dest_root.join(rel);
            if let Some(parent) = dest_file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &dest_file)?;
        }
    }
    Ok(())
}
