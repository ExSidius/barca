//! Dev-mode file watcher (`--watch`). Invalidates the cached DAG/plan whenever a
//! source file changes so `/assets` and `/plan` reflect edits without a restart.
//!
//! This is a local-development convenience only. In production the server is
//! started without `--watch`, no watcher thread is spawned, and the static
//! analysis cache simply persists for the process lifetime.

use crate::state::AppState;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;

/// Spawn a watcher over the parent directories of the configured source files.
/// The returned `RecommendedWatcher` must be kept alive for watching to continue.
pub fn spawn(state: AppState) -> notify::Result<RecommendedWatcher> {
    let cache = state.cache.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        // Any successful filesystem event invalidates the cache; it will be
        // rebuilt lazily on the next /assets or /plan request.
        if res.is_ok()
            && let Ok(mut c) = cache.write()
        {
            c.assets = None;
            c.plan = None;
        }
    })?;

    // Watch each file's parent directory (non-recursively). Editors frequently
    // replace files atomically, which directory-level watching catches reliably.
    let mut watched: Vec<std::path::PathBuf> = Vec::new();
    for f in &state.config.files {
        let dir = Path::new(f)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        if watched.contains(&dir) {
            continue;
        }
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        watched.push(dir);
    }

    Ok(watcher)
}
