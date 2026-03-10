use std::{fs, path::Path};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct BarcaConfig {
    pub python: PythonConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PythonConfig {
    pub modules: Vec<String>,
}

pub fn load_config(path: &Path) -> anyhow::Result<BarcaConfig> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}
