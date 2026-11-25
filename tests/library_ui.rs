//! Exercises the installed CLI entry point and HTML form endpoints, not an in-memory router.
use anyhow::{Context, Result, ensure};
use reqwest::{Client, StatusCode};
use rusqlite::Connection;
use rustix::process::{Pid, Signal, kill_process};
use std::{
    env, fs,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::time::{sleep, timeout};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(pid) = Pid::from_raw(self.0.id() as i32) {
            let _ = kill_process(pid, Signal::TERM);
        }
        let _ = self.0.wait();
    }
}
async fn serve(dir: &Path) -> Result<(Server, String)> {
    let bin = env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .find(|dir| dir.join("loom-serve").exists())
        .context("loom-serve not on PATH")?;
    fs::create_dir_all(dir.join("run"))?;
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_lum"))
            .arg("serve")
            .env("RUNTIME_DIRECTORY", dir.join("run"))
            .env("STATE_DIRECTORY", dir.join("state"))
            .env("LUM_RUNTIME_BIN", bin)
            .env("LUM_DEVICE", "task")
            .env("LUM_ADMIN_PORT", "0")
            .stderr(Stdio::from(fs::File::create(dir.join("log"))?))
            .spawn()?,
    );
    let url = timeout(Duration::from_secs(10), async {
        loop {
            let log = fs::read_to_string(dir.join("log")).unwrap();
            if let Some(url) = log
                .lines()
                .find_map(|line| line.strip_prefix("Lum library: "))
            {
                break url.trim_end_matches('/').to_owned();
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .with_context(|| fs::read_to_string(dir.join("log")).unwrap_or_default())?;
    Ok((server, url))
}
async fn until(client: &Client, url: &str, needle: &str) -> Result<()> {
    timeout(Duration::from_secs(15), async {
        loop {
            if client.get(url).send().await?.text().await?.contains(needle) {
                return Ok(());
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await?
}

#[tokio::test]
#[ignore = "requires native hrx-loom 0.3.0 loom-serve on PATH"]
async fn library_forms_chat_restore_and_remove() -> Result<()> {
    let root = tempfile::tempdir()?;
    let dir = root.path();
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let (first, url) = serve(dir).await?;
    until(&client, &url, "No models yet").await?;
    let blocked = client
        .post(format!("{url}/download"))
        .header("Origin", "http://untrusted.test")
        .send()
        .await?;
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        client
            .get(&url)
            .header("Host", "untrusted.test")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    // Seed a verified package instead of depending on the external HF catalog/weights.
    let model = dir.join("state/models/check");
    fs::create_dir(&model)?;
    for file in
        fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/loom-text"))?
    {
        let f = file?;
        fs::copy(f.path(), model.join(f.file_name()))?;
    }
    Connection::open(dir.join("state/library.sqlite"))?.execute("INSERT INTO models(alias,repo,preset,state) VALUES('check','loom-fun/fixture','check','installed')",[])?;
    assert_eq!(
        client
            .post(format!("{url}/models/check/start"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::SEE_OTHER
    );
    until(&client, &url, "Open playground").await?;
    let chat = client
        .post(format!("{url}/play/check"))
        .header("Origin", &url)
        .form(&[("tokens", "4"), ("history", "[]"), ("prompt", "a")])
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    ensure!(
        chat.contains("abab") && chat.contains("output tok/s"),
        "missing answer or usage: {chat}"
    );
    assert_eq!(
        client
            .post(format!("{url}/models/check/delete"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    drop(first);
    let (_second, url) = serve(dir).await?;
    until(&client, &url, "Open playground").await?;
    assert_eq!(
        client
            .post(format!("{url}/models/check/stop"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::SEE_OTHER
    );
    until(&client, &url, "Start model").await?;
    assert_eq!(
        client
            .post(format!("{url}/models/check/delete"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::SEE_OTHER
    );
    until(&client, &url, "No models yet").await?;
    assert!(!model.exists());
    Ok(())
}
