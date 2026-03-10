use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

pub const PROTOCOL_VERSION: &str = "0.1.1";

pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes.as_ref());
    hex::encode(hasher.finalize())
}

pub fn optional_file_hash(path: &Path) -> Option<String> {
    fs::read(path).ok().map(sha256_hex)
}

pub fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn slugify(parts: &[&str]) -> String {
    let joined = parts.join("-");
    let mut out = String::with_capacity(joined.len());
    let mut last_dash = false;
    for ch in joined.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
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

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Serialize)]
pub struct DefinitionHashPayload<'a> {
    pub module_source: &'a str,
    pub function_source: &'a str,
    pub decorator_metadata: &'a serde_json::Value,
    pub serializer_kind: &'a str,
    pub python_version: &'a str,
    pub uv_lock_hash: Option<&'a str>,
    pub protocol_version: &'a str,
}

pub fn compute_definition_hash(payload: &DefinitionHashPayload<'_>) -> anyhow::Result<String> {
    Ok(sha256_hex(serde_json::to_vec(payload)?))
}

pub fn repo_child(root: &Path, child: impl AsRef<Path>) -> PathBuf {
    root.join(child)
}
