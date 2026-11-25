//! Child ownership and the private native HTTP API (hrx-loom 0.3).
use std::{
    fs::Permissions, os::unix::fs::PermissionsExt, path::Path, process::Stdio, time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use reqwest::{Client, RequestBuilder, Response};
use rustix::process::{Pid, Signal, kill_process};
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{
    process::{Child, Command},
    time::{sleep, timeout},
};

const BASE: &str = "http://localhost"; // Routed over the child-specific Unix socket, never TCP.
const POLL: Duration = Duration::from_millis(50);

pub struct Engine {
    child: Child,
    pub api: Api,
    _dir: TempDir,
}

impl Engine {
    pub fn spawn_cpu(binary: &Path, temp_root: &Path) -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("lum-")
            .permissions(Permissions::from_mode(0o700))
            .tempdir_in(temp_root)?;
        let socket = dir.path().join("engine.sock");
        let client = Client::builder()
            .unix_socket(socket.clone())
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()?;
        let child = Command::new(binary)
            .args(["--device=task", "--pool_backing=fixed"])
            .arg(format!("--listen=unix:{}", socket.display()))
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("launch loom-serve (is it on PATH?)")?;
        Ok(Self {
            child,
            api: Api(client),
            _dir: dir,
        })
    }

    /// Observe child death even if the operation is waiting on readiness or HTTP.
    pub async fn run_until_exit<T>(&mut self, work: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::select! {
            biased;
            status = self.child.wait() => bail!("loom-serve exited unexpectedly: {}", status?),
            result = work => result,
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            let pid = self
                .child
                .id()
                .and_then(|n| Pid::from_raw(n as i32))
                .context("missing Loom PID")?;
            // The child has not been reaped, so its PID cannot be recycled here.
            match kill_process(pid, Signal::TERM) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => (),
                Err(e) => return Err(e.into()),
            }
            match timeout(Duration::from_secs(5), self.child.wait()).await {
                Ok(result) => {
                    result.context("reap loom-serve")?;
                }
                Err(_) => self
                    .child
                    .kill()
                    .await
                    .context("force-stop and reap loom-serve")?,
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Api(Client);

#[derive(Deserialize)]
struct Status {
    data: Vec<Model>,
}
#[derive(Deserialize)]
struct Model {
    id: String,
    loom: Live,
}
#[derive(Deserialize)]
struct Live {
    registration: String,
    status: Option<Admission>,
    error: Option<Value>,
}
#[derive(Deserialize)]
struct Admission {
    admission: String,
}

async fn checked(request: RequestBuilder) -> Result<Response> {
    let response = request.send().await?;
    if !response.status().is_success() {
        bail!(
            "Loom returned {}: {}",
            response.status(),
            response.text().await?
        );
    }
    Ok(response)
}

impl Api {
    pub async fn wait_empty(&self) -> Result<()> {
        timeout(Duration::from_secs(15), async {
            loop {
                if self
                    .0
                    .get(format!("{BASE}/readyz"))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    break;
                }
                sleep(POLL).await;
            }
        })
        .await
        .context("Loom did not become ready within 15 seconds")?;
        ensure!(
            self.status().await?.data.is_empty(),
            "new engine is not empty"
        );
        Ok(())
    }

    async fn status(&self) -> Result<Status> {
        Ok(checked(self.0.get(format!("{BASE}/v1/status")))
            .await?
            .json()
            .await?)
    }

    async fn wait_model(&self, alias: &str, registered: bool) -> Result<()> {
        timeout(Duration::from_secs(30), async {
            loop {
                let found = self
                    .status()
                    .await?
                    .data
                    .into_iter()
                    .find(|m| m.id == alias);
                match found.map(|m| m.loom) {
                    None if !registered => return Ok(()),
                    Some(m) if matches!(m.registration.as_str(), "failed" | "release_failed") => {
                        bail!("{alias}: {}: {:?}", m.registration, m.error)
                    }
                    Some(m)
                        if registered
                            && m.registration == "ready"
                            && m.status.as_ref().is_some_and(|s| s.admission == "open") =>
                    {
                        return Ok(());
                    }
                    _ => sleep(POLL).await,
                }
            }
        })
        .await
        .with_context(|| format!("timed out waiting for {alias} (registered={registered})"))?
    }

    pub async fn register(&self, alias: &str, package: &Path, preset: &str) -> Result<()> {
        let doc = json!({"name":alias,"base_directory":package,"model":{"path":"model.json","preset":preset},"definition":{"kind":"text"}});
        checked(self.0.post(format!("{BASE}/v1/models/register")).json(&doc)).await?;
        self.wait_model(alias, true).await
    }

    pub async fn remove(&self, alias: &str) -> Result<()> {
        checked(
            self.0
                .post(format!("{BASE}/v1/models/unregister"))
                .json(&json!({"model":alias,"mode":"drain"})),
        )
        .await?;
        self.wait_model(alias, false).await
    }

    pub async fn chat(&self, alias: &str, stream: bool) -> Result<Response> {
        checked(self.0.post(format!("{BASE}/v1/chat/completions")).json(&json!({"model":alias,"messages":[{"role":"user","content":"a"}],"stream":stream,"max_tokens":4}))).await
    }
}
