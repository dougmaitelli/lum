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
            .env("LUM_INFERENCE_PORT", "0")
            .stderr(Stdio::from(fs::File::create(dir.join("log"))?))
            .spawn()?,
    );
    address(server, dir, "Lum library: ").await
}
async fn address(server: Server, dir: &Path, prefix: &str) -> Result<(Server, String)> {
    let url = timeout(Duration::from_secs(10), async {
        loop {
            let log = fs::read_to_string(dir.join("log")).unwrap();
            if let Some(url) = log.lines().find_map(|line| line.strip_prefix(prefix)) {
                break url.trim_end_matches('/').to_owned();
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .with_context(|| fs::read_to_string(dir.join("log")).unwrap_or_default())?;
    Ok((server, url))
}
#[cfg(feature = "dev-ui")]
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

async fn until_state(
    client: &Client,
    url: &str,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> Result<()> {
    timeout(Duration::from_secs(15), async {
        loop {
            let res = client.get(format!("{url}/admin/state")).send().await?;
            if let Ok(value) = res.json().await
                && predicate(&value)
            {
                return Ok(());
            };
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await?
}
async fn check_inference(client: &Client, admin: &str, alias: &str) -> Result<()> {
    let issue = || {
        client
            .post(format!("{admin}/admin/keys"))
            .json(&serde_json::json!({"name":"integration"}))
    };
    assert_eq!(issue().send().await?.status(), StatusCode::FORBIDDEN);
    let r = issue()
        .header("Origin", admin)
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(r.headers()["cache-control"], "no-store");
    let issued: serde_json::Value = r.json().await?;
    let token = issued["secret"].as_str().context("issued key")?.to_owned();
    let info = client
        .get(format!("{admin}/admin/api"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(!info.contains(&token));
    let info: serde_json::Value = serde_json::from_str(&info)?;
    let base = info["base"].as_str().context("advertised endpoint")?;
    let id = info["keys"][0]["id"].as_i64().context("key id")?;
    assert!(!base.starts_with(&format!("{admin}/")));
    let models = format!("{base}/models");
    let chat = format!("{base}/chat/completions");
    assert_eq!(
        client.get(&models).send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .post(&chat)
            .bearer_auth("invalid")
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let list: serde_json::Value = client
        .get(&models)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(
        list["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == alias)
    );
    for streaming in [false, true] {
        let r=client.post(&chat).bearer_auth(&token).json(&serde_json::json!({"model":alias,"messages":[{"role":"user","content":"a"}],"max_tokens":4,"stream":streaming})).send().await?.error_for_status()?;
        if streaming {
            assert!(r.text().await?.contains("[DONE]"));
        } else {
            let v: serde_json::Value = r.json().await?;
            assert!(v["choices"][0]["message"]["content"].is_string());
        }
    }
    for path in [
        "/api",
        "/admin/keys",
        "/admin/api",
        "/v1/models/register",
        "/metrics",
    ] {
        assert_eq!(
            client
                .get(format!("{}{path}", base.strip_suffix("/v1").unwrap()))
                .bearer_auth(&token)
                .send()
                .await?
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    client
        .post(format!("{admin}/admin/keys/{id}/revoke"))
        .header("Origin", admin)
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        client
            .get(&models)
            .bearer_auth(&token)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}
#[cfg(feature = "dev-ui")]
#[tokio::test]
async fn isolated_preview_workflow() -> Result<()> {
    let root = tempfile::tempdir()?;
    let tmp = root.path().join("tmp");
    fs::create_dir(&tmp)?;
    let forbidden = root.path().join("real-state");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_lum"))
            .arg("dev")
            .env_clear()
            .env("PATH", "")
            .env("TMPDIR", &tmp)
            .env("STATE_DIRECTORY", &forbidden)
            .env("LUM_DEV_PORT", "0")
            .env("LUM_DEV_INFERENCE_PORT", "0")
            .stderr(Stdio::from(fs::File::create(root.path().join("log"))?))
            .spawn()?,
    );
    let (server, url) = address(server, root.path(), "Lum UI preview: ").await?;
    let client = Client::builder().no_proxy().build()?;
    until(&client, &url, "/assets/chat.js").await?;
    assert!(
        client
            .get(format!("{url}/assets/chat.js"))
            .send()
            .await?
            .error_for_status()?
            .content_length()
            .unwrap_or(0)
            > 0
    );
    let snapshot = client
        .get(format!("{url}/admin/state"))
        .send()
        .await?
        .text()
        .await?;
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot)?;
    assert_eq!(snapshot["mock"], true);
    assert_eq!(snapshot["online"], true);
    assert_eq!(
        snapshot["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["loaded"] == true)
            .count(),
        1
    );
    assert_eq!(
        client.get(format!("{url}/catalog")).send().await?.status(),
        StatusCode::NOT_FOUND
    );
    for alias in ["active-cancel", "queued-cancel"] {
        client
            .post(format!("{url}/admin/downloads"))
            .header("Origin", &url)
            .json(&serde_json::json!({"alias":alias,"repo":"loom-fun/example"}))
            .send()
            .await?
            .error_for_status()?;
        if alias == "active-cancel" {
            until_state(&client, &url, |s| s["progress"]["percent"].is_number()).await?;
        }
    }
    for alias in ["queued-cancel", "active-cancel"] {
        client
            .post(format!("{url}/admin/downloads/{alias}/cancel"))
            .header("Origin", &url)
            .send()
            .await?
            .error_for_status()?;
        until_state(&client, &url, |s| {
            !s["models"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["alias"] == alias)
        })
        .await?;
    }
    client
        .post(format!("{url}/admin/downloads"))
        .header("Origin", &url)
        .json(&serde_json::json!({"alias":"test","repo":"https://huggingface.co/another-publisher/example"}))
        .send()
        .await?
        .error_for_status()?;
    client
        .post(format!("{url}/admin/downloads"))
        .header("Origin", &url)
        .json(&serde_json::json!({"repo":"loom-fun/second"}))
        .send()
        .await?
        .error_for_status()?;
    until_state(&client, &url, |s| s["progress"]["percent"].is_number()).await?;
    until_state(&client, &url, |s| {
        s["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["state"] == "installed")
    })
    .await?;
    let change = |op: &str| {
        client
            .post(format!("{url}/admin/models/test/{op}"))
            .header("Origin", &url)
    };
    let denied = change("start").send().await?;
    assert_eq!(denied.status(), StatusCode::BAD_REQUEST);
    assert!(denied.text().await?.contains("models.max_loaded = 1"));
    client
        .post(format!("{url}/admin/models/code/stop"))
        .header("Origin", &url)
        .send()
        .await?
        .error_for_status()?;
    change("start").send().await?.error_for_status()?;
    let reply = client
        .post(format!("{url}/admin/chat/test"))
        .header("Origin", &url)
        .json(&serde_json::json!({"prompt":"hello","history":[],"tokens":32}))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(reply.contains("Mock reply from test: hello"));
    until(
        &client,
        &format!("{url}/metrics"),
        "lum_mock_requests_total 1",
    )
    .await?;
    check_inference(&client, &url, "test").await?;
    change("stop").send().await?.error_for_status()?;
    change("delete").send().await?.error_for_status()?;
    assert_eq!(
        client
            .post(format!("{url}/admin/models/test/delete"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    drop(server);
    assert!(!forbidden.exists());
    assert!(fs::read_dir(tmp)?.next().is_none());
    Ok(())
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
    until_state(&client, &url, |s| {
        s["models"].as_array().unwrap().is_empty()
    })
    .await?;
    let blocked = client
        .post(format!("{url}/admin/downloads"))
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
    let settings = |limit: i32| {
        client
            .post(format!("{url}/admin/models/check/settings"))
            .header("Origin", &url)
            .json(&serde_json::json!({"label":"Fixture","context_limit":limit}))
    };
    assert_eq!(settings(0).send().await?.status(), StatusCode::BAD_REQUEST);
    settings(80).send().await?.error_for_status()?;
    assert_eq!(
        client
            .post(format!("{url}/admin/models/check/start"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    until_state(&client, &url, |s| s["models"][0]["loaded"] == true).await?;
    assert_eq!(settings(64).send().await?.status(), StatusCode::BAD_REQUEST);
    let chat = client
        .post(format!("{url}/admin/chat/check"))
        .header("Origin", &url)
        .json(&serde_json::json!({"history":[],"prompt":"a"}))
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
            .post(format!("{url}/admin/models/check/delete"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    let stream = client
        .post(format!("{url}/admin/chat/check"))
        .header("Origin", &url)
        .header("Accept", "text/event-stream")
        .json(&serde_json::json!({"history":[],"prompt":"a"}))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    ensure!(
        stream.contains("[DONE]") && stream.contains("completion_tokens"),
        "Missing native stream completion or usage: {stream}"
    );
    check_inference(&client, &url, "check").await?;
    drop(first);
    let (_second, url) = serve(dir).await?;
    until_state(&client, &url, |s| {
        s["models"][0]["loaded"] == true
            && s["models"][0]["context_limit"] == 80
            && s["models"][0]["label"] == "Fixture"
    })
    .await?;
    assert_eq!(
        client
            .post(format!("{url}/admin/models/check/stop"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    until_state(&client, &url, |s| s["models"][0]["registered"] == false).await?;
    assert_eq!(
        client
            .post(format!("{url}/admin/models/check/delete"))
            .header("Origin", &url)
            .send()
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    until_state(&client, &url, |s| {
        s["models"].as_array().unwrap().is_empty()
    })
    .await?;
    assert!(!model.exists());
    Ok(())
}
