use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use barca_core::models::{InspectResponse, InspectedAsset, WorkerResponse};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{emit_log, JobLogEntry, JobLogLevel};

/// Directories to skip when auto-discovering Python modules.
const SKIP_DIRS: &[&str] = &[".venv", "__pycache__", ".git", ".barca", ".barcafiles", "build", "dist", "node_modules", "target", "tmp"];

/// Walk `root` for `.py` files that reference `barca` and convert them to
/// importable dotted module names.  Called automatically when no explicit
/// modules are passed to `inspect_modules`.
fn discover_barca_modules(root: &Path) -> Vec<String> {
    let mut modules = Vec::new();
    walk_py_files(root, root, &mut modules);
    modules.sort();
    modules
}

fn walk_py_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
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
        // Skip hidden dirs and known non-project dirs
        if name.starts_with('.') || SKIP_DIRS.contains(&name) {
            continue;
        }
        if path.is_dir() {
            walk_py_files(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("py") {
            // Only consider files whose content references barca
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if !content.contains("barca") {
                continue;
            }
            // Convert path to dotted module name relative to root
            if let Some(module_name) = path_to_module(root, &path) {
                out.push(module_name);
            }
        }
    }
}

fn path_to_module(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let without_ext = rel.with_extension("");
    // __init__.py -> parent package name
    let module_path = if without_ext.file_name()?.to_str()? == "__init__" {
        without_ext.parent()?.to_path_buf()
    } else {
        without_ext
    };
    let parts: Vec<&str> = module_path.components().filter_map(|c| c.as_os_str().to_str()).collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("."))
}

#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait PythonBridge: Send + Sync {
    async fn inspect_modules(&self, modules: &[String]) -> anyhow::Result<Vec<InspectedAsset>>;

    async fn materialize_asset(
        &self,
        module_path: &str,
        function_name: &str,
        output_dir: &Path,
        job_id: i64,
        log_tx: broadcast::Sender<JobLogEntry>,
        asset_id: i64,
        input_kwargs_json: Option<&str>,
    ) -> anyhow::Result<WorkerResponse>;
}

#[derive(Clone)]
pub struct UvPythonBridge {
    repo_root: PathBuf,
}

impl UvPythonBridge {
    pub fn new(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }
}

#[async_trait]
impl PythonBridge for UvPythonBridge {
    async fn inspect_modules(&self, modules: &[String]) -> anyhow::Result<Vec<InspectedAsset>> {
        // If no explicit modules given, auto-discover .py files that reference barca.
        let effective: Vec<String> = if modules.is_empty() { discover_barca_modules(&self.repo_root) } else { modules.to_vec() };

        // Nothing to inspect — return early without spawning a subprocess.
        if effective.is_empty() {
            return Ok(Vec::new());
        }

        let mut command = Command::new("uv");
        command.current_dir(&self.repo_root);
        command.arg("run").arg("python").arg("-m").arg("barca.inspect");
        for module in &effective {
            command.arg("--module").arg(module);
        }
        command.env("PYTHONPATH", self.repo_root.display().to_string());
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = command.output().await.context("failed to run python inspector")?;
        if !output.status.success() {
            return Err(anyhow!("python inspector failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        let response: InspectResponse = serde_json::from_slice(&output.stdout).context("failed to decode inspector output")?;
        Ok(response.assets)
    }

    async fn materialize_asset(
        &self,
        module_path: &str,
        function_name: &str,
        output_dir: &Path,
        job_id: i64,
        log_tx: broadcast::Sender<JobLogEntry>,
        asset_id: i64,
        input_kwargs_json: Option<&str>,
    ) -> anyhow::Result<WorkerResponse> {
        let mut command = Command::new("uv");
        command.current_dir(&self.repo_root);
        command
            .arg("run")
            .arg("python")
            .arg("-m")
            .arg("barca.worker")
            .arg("--module")
            .arg(module_path)
            .arg("--function")
            .arg(function_name)
            .arg("--output-dir")
            .arg(output_dir);
        if let Some(kwargs) = input_kwargs_json {
            command.arg("--input-kwargs").arg(kwargs);
        }
        command.env("PYTHONPATH", self.repo_root.display().to_string());
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = command.spawn().context("failed to spawn python worker")?;
        let pid = child.id().unwrap_or(0);
        info!(job_id, pid, "python worker started");
        emit_log(&log_tx, asset_id, job_id, JobLogLevel::Info, format!("Python worker started (pid:{pid})"));

        // Stream stdout line-by-line.
        let stdout = child.stdout.take().expect("stdout was piped");
        let stdout_log_tx = log_tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                info!(job_id, pid, "{line}");
                emit_log(&stdout_log_tx, asset_id, job_id, JobLogLevel::Output, &line);
            }
        });

        // Stream stderr line-by-line.
        let stderr = child.stderr.take().expect("stderr was piped");
        let stderr_log_tx = log_tx;
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                warn!(job_id, pid, "{line}");
                emit_log(&stderr_log_tx, asset_id, job_id, JobLogLevel::Warn, &line);
            }
        });

        let status = child.wait().await.context("failed to wait for python worker")?;
        let _ = stdout_task.await;
        let _ = stderr_task.await;

        let result_path = output_dir.join("result.json");
        let result_bytes = fs::read(&result_path).with_context(|| format!("worker did not produce result.json (exit status: {status})"))?;
        let response: WorkerResponse = serde_json::from_slice(&result_bytes).context("failed to decode worker result")?;

        if !status.success() || !response.ok {
            let error = response.error.clone().unwrap_or_else(|| format!("worker exited with {status}"));
            return Err(anyhow!(error));
        }

        Ok(response)
    }
}
