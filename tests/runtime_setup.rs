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
            .env("LUM_RUNTIME_BIN", &bin)
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
            let r = http.get(&origin).send().await.unwrap();
            if r.status() == StatusCode::SERVICE_UNAVAILABLE {
                assert!(r.text().await.unwrap().contains("Retry setup"));
                break;
            }
            sleep(Duration::from_millis(30)).await;
        }
    })
    .await?;
    let retry = format!("{origin}/runtime/retry");
    assert_eq!(
        http.post(&retry)
            .header("Origin", "http://untrusted.test")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    // Successful checks on retry advance to launching the engine. /bin/true then
    // exits immediately, so the supervisor reports child exit, not setup failure.
    for tool in ["loom-fetch", "loom-serve"] {
        symlink("/bin/true", bin.join(tool))?;
    }
    assert_eq!(
        http.post(retry)
            .header("Origin", &origin)
            .send()
            .await?
            .status(),
        StatusCode::SEE_OTHER
    );
    timeout(Duration::from_secs(5), async {
        while child.0.try_wait().unwrap().is_none() {
            sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .context("retry did not reach engine launch")?;
    assert!(fs::read_to_string(log)?.contains("loom-serve exited"));
    assert!(tmp.path().join("data/lum/library.sqlite").is_file());
    Ok(())
}
