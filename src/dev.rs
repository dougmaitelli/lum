//! Mock Loom runtime for UI development. Compiled only with `dev-ui`; no runtime provisioning or child processes.
use crate::{
    config::Listener,
    library::Library,
    native::Native,
    ui::{self, App, Setup},
};
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, UnixListener},
    signal::unix::{SignalKind, signal},
    sync::{Mutex, Notify, watch},
};

const REV: &str = "1a2b3c4d5e6f0000000000000000000000000000";
async fn stage(lib: &Library, a: &str) -> Result<()> {
    let staging = lib.staging(a);
    tokio::fs::create_dir(&staging).await?;
    tokio::fs::write(staging.join("model.json"), b"{\"mock\":true}\n").await?;
    Ok(())
}
#[derive(Default)]
struct Engine {
    ready: BTreeSet<String>,
    requests: u64,
}
type Shared = Arc<Mutex<Engine>>;
fn mock_engine(state: Shared) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/models/register", post(register))
        .route("/v1/models/unregister", post(unregister))
        .route("/v1/chat/completions", post(chat))
        .route("/metrics", get(metrics))
        .with_state(state)
}
async fn status(State(s): State<Shared>) -> Json<Value> {
    let engine = s.lock().await;
    let data: Vec<_> = engine
        .ready
        .iter()
        .map(|a| json!({"id":a,"loom":{"registration":"ready","status":{"admission":"open"}}}))
        .collect();
    Json(json!({"data":data}))
}
#[derive(serde::Deserialize)]
struct Register {
    name: String,
}
async fn register(State(s): State<Shared>, Json(req): Json<Register>) -> StatusCode {
    s.lock().await.ready.insert(req.name);
    StatusCode::ACCEPTED
}
#[derive(serde::Deserialize)]
struct Unregister {
    model: String,
}
async fn unregister(State(s): State<Shared>, Json(req): Json<Unregister>) -> StatusCode {
    s.lock().await.ready.remove(&req.model);
    StatusCode::ACCEPTED
}
async fn metrics(State(s): State<Shared>) -> String {
    format!(
        "# Mock runtime; no inference performed.\n# TYPE lum_mock_requests_total counter\nlum_mock_requests_total {}\n",
        s.lock().await.requests
    )
}
async fn chat(
    State(s): State<Shared>,
    Json(v): Json<Value>,
) -> Result<axum::response::Response, StatusCode> {
    use axum::response::IntoResponse;
    let mut s = s.lock().await;
    let a = v["model"].as_str().ok_or(StatusCode::BAD_REQUEST)?;
    if !s.ready.contains(a) {
        return Err(StatusCode::NOT_FOUND);
    }
    let message = v["messages"]
        .as_array()
        .and_then(|m| m.last())
        .and_then(|m| m["content"].as_str())
        .ok_or(StatusCode::BAD_REQUEST)?;
    s.requests += 1;
    let text = format!("Mock reply from {a}: {message}");
    let id = format!("chatcmpl-mock-{}", s.requests);
    if v["stream"] == true {
        let chunk = |delta: Value, finish: Value| json!({"id":id,"object":"chat.completion.chunk","created":0,"model":a,"choices":[{"index":0,"delta":delta,"finish_reason":finish}]});
        return Ok((
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                chunk(json!({"role":"assistant","content":text}), Value::Null),
                chunk(json!({}), json!("stop"))
            ),
        )
            .into_response());
    }
    Ok(Json(json!({"id":id,"object":"chat.completion","created":0,"model":a,"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":text}}]})).into_response())
}
#[tokio::main(flavor = "current_thread")]
pub async fn run(hot_reload: bool) -> Result<()> {
    let port = std::env::var("LUM_DEV_PORT")
        .unwrap_or_else(|_| "9399".into())
        .parse::<u16>()
        .context("Invalid LUM_DEV_PORT")?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, if hot_reload { 0 } else { port }))
        .await
        .context("Bind UI preview; change LUM_DEV_PORT if occupied")?;
    let dir = tempfile::Builder::new()
        .prefix("lum-ui-")
        .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .tempdir()?;
    let lib = Library::open(dir.path())?;
    for (a, r) in [
        ("code", "loom-fun/dev-code-3b"),
        ("instruct", "loom-fun/dev-instruct-1b"),
    ] {
        lib.reserve(a, r, "chat")?;
        stage(&lib, a).await?;
        tokio::fs::rename(lib.staging(a), lib.package(a)).await?;
        lib.revision(a, REV)?;
        lib.finish(a, "installed", "")?;
    }
    lib.desired("code", true)?;
    let socket = dir.path().join("mock.sock");
    let private = UnixListener::bind(&socket)?;
    let engine = Engine {
        ready: BTreeSet::from(["code".into()]),
        requests: 0,
    };
    let mocked = mock_engine(Arc::new(Mutex::new(engine)));
    let api_port = std::env::var("LUM_DEV_INFERENCE_PORT")
        .unwrap_or_else(|_| "9400".into())
        .parse::<u16>()
        .context("Invalid LUM_DEV_INFERENCE_PORT")?;
    let api = TcpListener::bind((Ipv4Addr::LOCALHOST, api_port))
        .await
        .context("Bind mock inference listener; change LUM_DEV_INFERENCE_PORT if occupied")?;
    let native = Native::new(&socket)?;
    let inference = crate::inference::router(lib.clone(), native.clone());
    let app = App::preview(lib, native)?;
    let addr = listener.local_addr()?;
    let ui_port = if hot_reload {
        anyhow::ensure!(port != 0, "Set a fixed LUM_DEV_PORT for Vite");
        port
    } else {
        addr.port()
    };
    let mut vite = if hot_reload {
        Some(
            tokio::process::Command::new("node")
                .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/web"))
                .env("LUM_DEV_BACKEND", format!("http://{addr}"))
                .args([
                    "node_modules/vite/bin/vite.js",
                    "--port",
                    &ui_port.to_string(),
                ])
                .kill_on_drop(true)
                .spawn()
                .context("Start Vite (install web dependencies with npm ci --prefix web)")?,
        )
    } else {
        None
    };
    let cfg = Listener {
        max_loaded: 1,
        inference: api.local_addr()?,
        host: addr.ip(),
        port: ui_port,
        name: None,
    };
    let (_tx, rx) = watch::channel(Setup::Ready);
    let router = ui::router(app, cfg, rx, Arc::new(Notify::new()), "Mock runtime".into());
    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    eprintln!("Lum UI preview: http://127.0.0.1:{ui_port}/");
    let wait_vite = async {
        match &mut vite {
            Some(child) => child.wait().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! { s=wait_vite=>anyhow::bail!("Vite exited: {}",s?),r=axum::serve(private,mocked)=>r?,r=axum::serve(api,inference)=>r?, r=axum::serve(listener,router.into_make_service_with_connect_info::<ui::Destination>())=>r?,_=term.recv()=>(),_=interrupt.recv()=>() }
    Ok(())
}
pub async fn download(
    lib: &Library,
    alias: &str,
    repo: &str,
    log: &Mutex<Vec<u8>>,
) -> Result<String> {
    *log.lock().await = format!("Simulating download of {repo} (no network access)").into_bytes();
    for step in 0..10 {
        log.lock().await.extend_from_slice(
            format!(
                "\nloom-fetch  Download  [mock] {}%  fixture.bin\n",
                step * 10
            )
            .as_bytes(),
        );
        tokio::time::sleep(Duration::from_millis(350)).await;
    }
    stage(lib, alias).await?;
    log.lock()
        .await
        .extend_from_slice(b"\nFixture installed. No weights downloaded.");
    Ok(REV.into())
}
