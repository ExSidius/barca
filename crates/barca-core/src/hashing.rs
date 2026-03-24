use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

pub const PROTOCOL_VERSION: &str = "0.2.0";

/// Directories to skip when walking the project for codebase hashing.
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

/// Compute a merkle-tree-style hash of the entire Python codebase.
///
/// Walks all `.py` files under `root` (skipping known non-project dirs),
/// hashes each file's content keyed by its relative path, sorts the leaf
/// hashes, and produces a single root hash. Also includes `uv.lock` if
/// present, so dependency changes are captured.
pub fn compute_codebase_hash(root: &Path) -> anyhow::Result<String> {
    let mut leaf_hashes: Vec<(String, String)> = Vec::new();

    // Include uv.lock first (if it exists)
    let uv_lock_path = root.join("uv.lock");
    if uv_lock_path.is_file() {
        if let Ok(bytes) = fs::read(&uv_lock_path) {
            leaf_hashes.push(("uv.lock".to_string(), sha256_hex(&bytes)));
        }
    }

    // Walk all .py files
    walk_py_for_hash(root, root, &mut leaf_hashes);

    // Sort by path for determinism
    leaf_hashes.sort_by(|a, b| a.0.cmp(&b.0));

    // Compute root hash from sorted leaf hashes
    let mut hasher = Sha256::new();
    for (path, hash) in &leaf_hashes {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }
    Ok(hex::encode(hasher.finalize()))
}

fn walk_py_for_hash(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('.') || SKIP_DIRS.contains(&name) {
            continue;
        }
        if path.is_dir() {
            walk_py_for_hash(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("py") {
            if let Ok(bytes) = fs::read(&path) {
                let rel = relative_path(root, &path);
                out.push((rel, sha256_hex(&bytes)));
            }
        }
    }
}

pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes.as_ref());
    hex::encode(hasher.finalize())
}

pub fn optional_file_hash(path: &Path) -> Option<String> {
    fs::read(path).ok().map(sha256_hex)
}

pub fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

pub fn slugify(parts: &[&str]) -> String {
    let joined = parts.join("-");
    let mut out = String::with_capacity(joined.len());
    let mut last_dash = false;
    for ch in joined.chars() {
        let mapped = if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' };
        if mapped == '-' {
            if !last_dash {
                out.push(mapped);
            }
            last_dash = true;
        } else {
            out.push(mapped);
            last_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn now_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

#[derive(Serialize)]
pub struct DefinitionHashPayload<'a> {
    pub codebase_hash: &'a str,
    pub function_source: &'a str,
    pub decorator_metadata: &'a serde_json::Value,
    pub serializer_kind: &'a str,
    pub python_version: &'a str,
    pub protocol_version: &'a str,
}

pub fn compute_definition_hash(payload: &DefinitionHashPayload<'_>) -> anyhow::Result<String> {
    Ok(sha256_hex(serde_json::to_vec(payload)?))
}

/// Compute a run_hash that incorporates upstream materialization IDs and
/// an optional partition key. For leaf assets with no inputs or partitions,
/// callers should use `definition_hash` directly.
pub fn compute_run_hash(definition_hash: &str, upstream_materialization_ids: &[i64], partition_key_json: Option<&str>) -> String {
    #[derive(Serialize)]
    struct RunHashPayload<'a> {
        definition_hash: &'a str,
        upstream_materialization_ids: &'a [i64],
        partition_key_json: Option<&'a str>,
    }
    let payload = RunHashPayload {
        definition_hash,
        upstream_materialization_ids,
        partition_key_json,
    };
    sha256_hex(serde_json::to_vec(&payload).expect("run hash serialization"))
}

pub fn repo_child(root: &Path, child: impl AsRef<Path>) -> PathBuf {
    root.join(child)
}
