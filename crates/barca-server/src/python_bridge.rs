use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{anyhow, Context};
use barca_core::models::{InspectResponse, InspectedAsset, WorkerResponse};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{emit_log, JobLogEntry, JobLogLevel};

#[derive(Clone)]
pub struct PythonBridge {
    repo_root: PathBuf,
}

impl PythonBridge {
    pub fn new(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }

    pub async fn inspect_modules(&self, modules: &[String]) -> anyhow::Result<Vec<InspectedAsset>> {
        if modules.is_empty() {
            return Ok(Vec::new());
        }
        let mut command = Command::new("uv");
        command.current_dir(&self.repo_root);
        command
            .arg("run")
            .arg("python")
            .arg("-m")
            .arg("barca.inspect");
        for module in modules {
            command.arg("--module").arg(module);
        }
        command.env("PYTHONPATH", self.repo_root.display().to_string());
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = command
            .output()
            .await
            .context("failed to run python inspector")?;
        if !output.status.success() {
            return Err(anyhow!(
                "python inspector failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let response: InspectResponse =
            serde_json::from_slice(&output.stdout).context("failed to decode inspector output")?;
        Ok(response.assets)
    }

    pub async fn materialize_asset(
        &self,
        module_path: &str,
        function_name: &str,
        output_dir: &Path,
        job_id: i64,
        log_tx: broadcast::Sender<JobLogEntry>,
        asset_id: i64,
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
        command.env("PYTHONPATH", self.repo_root.display().to_string());
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = command.spawn().context("failed to spawn python worker")?;
        let pid = child.id().unwrap_or(0);
        info!(job_id, pid, "python worker started");
        emit_log(
            &log_tx,
            asset_id,
            job_id,
            JobLogLevel::Info,
            format!("Python worker started (pid:{pid})"),
        );

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

        let status = child
            .wait()
            .await
            .context("failed to wait for python worker")?;
        let _ = stdout_task.await;
        let _ = stderr_task.await;

        let result_path = output_dir.join("result.json");
        let result_bytes = fs::read(&result_path).with_context(|| {
            format!("worker did not produce result.json (exit status: {status})")
        })?;
        let response: WorkerResponse =
            serde_json::from_slice(&result_bytes).context("failed to decode worker result")?;

        if !status.success() || !response.ok {
            let error = response
                .error
                .clone()
                .unwrap_or_else(|| format!("worker exited with {status}"));
            return Err(anyhow!(error));
        }

        Ok(response)
    }
}
