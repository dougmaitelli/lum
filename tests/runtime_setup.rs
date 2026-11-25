use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use std::{
    fs,
    os::unix::fs::symlink,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::time::{sleep, timeout};

// No network and no SDK: probe failure must leave a usable setup/retry UI.
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[tokio::test]
async fn foreground_setup_failure_is_recoverable_without_systemd() -> Result<()> {
    exercise_retry(None).await
}
#[tokio::test]
#[ignore = "requires the pinned CPU Loom SDK on PATH"]
async fn startup_retry_recovers_on_the_same_listener() -> Result<()> {
    let bin = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .find(|p| p.join("loom-serve").is_file())
        .context("missing SDK")?;
    exercise_retry(Some(&bin)).await
}
async fn exercise_retry(real: Option<&std::path::Path>) -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let log = tmp.path().join("log");
    let bin = tmp.path().join("external");
    fs::create_dir(&bin)?;
    let mut child = Process(
        Command::new(env!("CARGO_BIN_EXE_lum"))
            .arg("run")
            .env_remove("STATE_DIRECTORY")
            .env_remove("RUNTIME_DIRECTORY")
            .env("XDG_DATA_HOME", tmp.path().join("data"))
            .env("XDG_RUNTIME_DIR", tmp.path().join("run"))
            .env("LUM_ADMIN_PORT", "0")
            .env("LUM_INFERENCE_PORT", "0")
            .env("LUM_RUNTIME_BIN", &bin)
            .env("LUM_DEVICE", "task")
            .stderr(Stdio::from(fs::File::create(&log)?))
            .spawn()?,
    );
    let origin = timeout(Duration::from_secs(10), async {
        loop {
            if let Some(u) = fs::read_to_string(&log)
                .unwrap()
                .lines()
                .find_map(|s| s.strip_prefix("Lum setup: "))
            {
                break u.trim_end_matches('/').to_owned();
            }
            sleep(Duration::from_millis(30)).await;
        }
    })
    .await?;
    let http = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    timeout(Duration::from_secs(5), async {
        loop {
            let r: serde_json::Value = http
                .get(format!("{origin}/admin/runtime"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            if r["setup"]["phase"] == "failed" {
                assert!(r["setup"]["error"].is_string());
                break;
            }
            sleep(Duration::from_millis(30)).await;
        }
    })
    .await?;
    for path in [
        "/",
        "/api",
        "/favicon.svg",
        "/assets/index.css",
        "/assets/chat.js",
    ] {
        http.get(format!("{origin}{path}"))
            .send()
            .await?
            .error_for_status()?;
    }
    assert_eq!(
        http.get(format!("{origin}/admin/api"))
            .send()
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let retry = format!("{origin}/admin/runtime/retry");
    assert_eq!(
        http.post(&retry)
            .header("Origin", "http://untrusted.test")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    // Successful checks on retry advance to launching the engine. /bin/true then
    // exits immediately. The same admin listener must still serve retries.
    for tool in ["loom-fetch", "loom-serve"] {
        symlink("/bin/true", bin.join(tool))?;
    }
    assert_eq!(
        http.post(&retry)
            .header("Origin", &origin)
            .send()
            .await?
            .status(),
        StatusCode::ACCEPTED
    );
    timeout(Duration::from_secs(5), async {
        loop {
            let state: serde_json::Value = http
                .get(format!("{origin}/admin/runtime"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            if state["setup"]["error"]
                .as_str()
                .is_some_and(|s| s.contains("loom-serve exited"))
            {
                break;
            }
            sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .context("retry did not reach engine launch")?;
    assert!(fs::read_to_string(log)?.contains("loom-serve exited"));
    if let Some(real) = real {
        fs::remove_file(bin.join("loom-serve"))?;
        symlink(real.join("loom-serve"), bin.join("loom-serve"))?;
    }
    assert_eq!(
        http.post(retry)
            .header("Origin", &origin)
            .send()
            .await?
            .status(),
        StatusCode::ACCEPTED
    );
    if real.is_some() {
        timeout(Duration::from_secs(10), async {
            loop {
                let state: serde_json::Value = http
                    .get(format!("{origin}/admin/runtime"))
                    .send()
                    .await?
                    .json()
                    .await?;
                if state["setup"]["phase"] == "ready" {
                    return Ok::<_, anyhow::Error>(());
                }
                sleep(Duration::from_millis(40)).await;
            }
        })
        .await??;
    }
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.0.id() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )?;
    assert!(child.0.wait()?.success());
    assert!(tmp.path().join("data/lum/library.sqlite").is_file());
    Ok(())
}
