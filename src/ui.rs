mod presets;
mod state;

use crate::{
    library::{Library, Model, slug, valid_repo},
    native::Native,
    runtime::Runtime,
};
use anyhow::{Context, Result, bail, ensure};
use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use reqwest::Client;
use serde::Deserialize;
use std::{
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, sync::Mutex};

#[derive(Clone)]
enum Packages {
    LoomFetch(Runtime),
    #[cfg(feature = "dev-ui")]
    Fixtures,
}
#[derive(Clone)]
pub struct App {
    pub lib: Library,
    pub engine: Native,
    packages: Packages,
    http: Client,
    gate: Arc<Mutex<()>>,
    worker: Arc<Mutex<()>>,
    log: Arc<Mutex<Vec<u8>>>,
}
impl App {
    pub fn new(lib: Library, engine: Native, fetch: Runtime) -> Result<Self> {
        Self::with_packages(lib, engine, Packages::LoomFetch(fetch))
    }
    fn with_packages(lib: Library, engine: Native, packages: Packages) -> Result<Self> {
        let http = Client::builder()
            .user_agent("Lum local model library")
            .timeout(Duration::from_secs(25))
            .build()?;
        Ok(Self {
            http,
            lib,
            engine,
            packages,
            gate: Default::default(),
            worker: Default::default(),
            log: Default::default(),
        })
    }
    #[cfg(feature = "dev-ui")]
    pub fn preview(lib: Library, engine: Native) -> Result<Self> {
        Self::with_packages(lib, engine, Packages::Fixtures)
    }
    fn mocked(&self) -> bool {
        match &self.packages {
            Packages::LoomFetch(_) => false,
            #[cfg(feature = "dev-ui")]
            Packages::Fixtures => true,
        }
    }
    fn model(&self, a: &str) -> Result<Model> {
        ensure!(slug(a), "Invalid alias");
        self.lib
            .list()?
            .into_iter()
            .find(|m| m.alias == a)
            .context("Unknown model alias")
    }
}
struct Problem(anyhow::Error);
impl From<anyhow::Error> for Problem {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}
impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, format!("{:#}", self.0)).into_response()
    }
}
type Ui<T> = std::result::Result<T, Problem>;

#[derive(Clone, serde::Serialize)]
#[serde(tag = "phase", content = "error", rename_all = "lowercase")]
pub enum Setup {
    Preparing,
    Failed(String),
    Ready,
}

/// Validate IP Host headers against the connection's actual destination even
/// when listening on a wildcard. Do not trust forwarded or client-provided IPs.
#[derive(Clone, Copy)]
pub struct Destination(Option<std::net::IpAddr>);
impl
    axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, tokio::net::TcpListener>>
    for Destination
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, tokio::net::TcpListener>) -> Self {
        Self(stream.io().local_addr().ok().map(|s| s.ip()))
    }
}
pub fn router(
    app: App,
    listener: crate::config::Listener,
    phase: tokio::sync::watch::Receiver<Setup>,
    retry: Arc<tokio::sync::Notify>,
    runtime: String,
) -> Router {
    let gate = phase.clone();
    Router::new()
        .route("/admin/api", get(api_info))
        .route("/admin/keys", post(create_key))
        .route("/admin/keys/{id}/revoke", post(revoke_key))
        .layer(Extension(listener.inference))
        .route("/admin/catalog", get(catalog))
        .route("/admin/presets", get(presets::list))
        .route("/admin/state", get(state::status))
        .route("/admin/models/{alias}/{op}", post(model_action))
        .route("/admin/chat/{alias}", post(chat))
        .route("/admin/downloads", post(download))
        .route("/metrics", get(metrics))
        .route_layer(middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let ready = matches!(*gate.borrow(), Setup::Ready);
                async move {
                    if ready {
                        next.run(req).await
                    } else {
                        (StatusCode::SERVICE_UNAVAILABLE, "Runtime not ready").into_response()
                    }
                }
            },
        ))
        // Bootstrap and static assets remain available even if the SDK installation failed.
        .route("/admin/runtime", get(runtime_status))
        .route("/admin/runtime/retry", post(runtime_retry))
        .layer(Extension(RuntimeStatus {
            phase,
            retry,
            runtime,
        }))
        .route("/", get(index))
        .route("/api", get(index))
        .route("/chat/{alias}", get(index))
        .route("/remove/{alias}", get(index))
        .route("/assets/chat.js", get(chat_bundle))
        .route("/assets/index.css", get(chat_css))
        .layer(DefaultBodyLimit::max(256 * 1024))
        .layer(middleware::from_fn(move |req, next| {
            protect(req, next, listener.clone())
        }))
        .with_state(app)
}

#[derive(Clone)]
struct RuntimeStatus {
    phase: tokio::sync::watch::Receiver<Setup>,
    retry: Arc<tokio::sync::Notify>,
    runtime: String,
}
async fn runtime_status(Extension(s): Extension<RuntimeStatus>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"runtime":s.runtime,"setup":s.phase.borrow().clone()}))
}
async fn runtime_retry(Extension(s): Extension<RuntimeStatus>) -> StatusCode {
    if matches!(*s.phase.borrow(), Setup::Failed(_)) {
        s.retry.notify_one();
        StatusCode::ACCEPTED
    } else {
        StatusCode::CONFLICT
    }
}
async fn chat_bundle() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript")],
        include_str!(concat!(env!("OUT_DIR"), "/ui/assets/chat.js")),
    )
}
async fn chat_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css")],
        include_str!(concat!(env!("OUT_DIR"), "/ui/assets/index.css")),
    )
}
async fn protect(req: Request<Body>, next: Next, listener: crate::config::Listener) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let dest = req
        .extensions()
        .get::<axum::extract::ConnectInfo<Destination>>()
        .and_then(|v| v.0.0);
    let allowed = host
        .parse::<axum::http::uri::Authority>()
        .ok()
        .is_some_and(|a| {
            let name = a.host();
            if a.as_str() != format!("{name}:{}", listener.port)
                && !(listener.port == 80 && a.as_str() == name)
            {
                return false;
            }
            let ip = name
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .ok();
            (ip.is_some() && ip == dest)
                || (name.eq_ignore_ascii_case("localhost")
                    && dest.is_some_and(|ip| ip.is_loopback()))
                || listener
                    .name
                    .as_ref()
                    .is_some_and(|n| name.eq_ignore_ascii_case(n))
        });
    let read_only = matches!(*req.method(), Method::GET | Method::HEAD);
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok());
    if !allowed || (!read_only && origin != Some(format!("http://{host}").as_str())) {
        return (
            StatusCode::FORBIDDEN,
            "Use the server IP or configured api.name, and a same-origin form.",
        )
            .into_response();
    }
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY,"default-src 'none'; style-src 'self'; script-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'".parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
}
async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!(concat!(env!("OUT_DIR"), "/ui/index.html")))
}
async fn model_action(
    State(app): State<App>,
    Path((a, op)): Path<(String, String)>,
) -> Ui<StatusCode> {
    apply(&app, &a, &op).await?;
    Ok(StatusCode::NO_CONTENT)
}
fn api_base(addr: std::net::SocketAddr, headers: &axum::http::HeaderMap) -> Result<String> {
    if !addr.ip().is_unspecified() {
        return Ok(format!("http://{addr}/v1"));
    }
    // The outer admin Host middleware has already checked this authority.
    let host = headers
        .get(header::HOST)
        .context("Missing Host")?
        .to_str()?
        .parse::<axum::http::uri::Authority>()?;
    Ok(format!("http://{}:{}/v1", host.host(), addr.port()))
}
async fn api_info(
    State(app): State<App>,
    Extension(addr): Extension<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
) -> Ui<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({"base":api_base(addr,&headers)?,"keys":app.lib.keys()?}),
    ))
}
#[derive(Deserialize)]
struct KeyForm {
    name: String,
}
async fn create_key(State(app): State<App>, Json(f): Json<KeyForm>) -> Ui<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({"secret":app.lib.create_key(&f.name)?}),
    ))
}
async fn revoke_key(State(app): State<App>, Path(id): Path<i64>) -> Ui<StatusCode> {
    app.lib.revoke_key(id)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn catalog(State(app): State<App>) -> Ui<Json<Vec<String>>> {
    #[derive(Deserialize)]
    struct Item {
        id: String,
    }
    let result = app
        .http
        .get("https://huggingface.co/api/models?author=loom-fun&limit=500")
        .send()
        .await
        .context("Catalog unavailable; paste a Hugging Face model link instead")?
        .error_for_status()
        .context("Hugging Face catalog request failed")?
        .json::<Vec<Item>>()
        .await
        .context("Invalid catalog response")?;
    let mut ids: Vec<_> = result
        .into_iter()
        .map(|i| i.id)
        .filter(|s| s.starts_with("loom-fun/") && valid_repo(s))
        .collect();
    ids.sort();
    Ok(Json(ids))
}
#[derive(Deserialize)]
struct Download {
    #[serde(default)]
    alias: String,
    repo: String,
    #[serde(default)]
    preset: String,
}
fn repository(input: &str) -> Result<String> {
    let s = input.trim().trim_end_matches('/');
    if valid_repo(s) {
        return Ok(s.to_owned());
    }
    let path = s
        .strip_prefix("https://huggingface.co/")
        .context("Paste an HTTPS huggingface.co model link or owner/repository")?
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let mut parts = path.split('/');
    let repo = format!(
        "{}/{}",
        parts.next().unwrap_or(""),
        parts.next().unwrap_or("")
    );
    ensure!(
        valid_repo(&repo) && matches!(parts.next(), None | Some("tree" | "blob" | "resolve")),
        "Expected a Hugging Face model repository link"
    );
    Ok(repo)
}
async fn download(State(app): State<App>, Json(mut d): Json<Download>) -> Ui<StatusCode> {
    d.repo = repository(&d.repo)?;
    if d.alias.trim().is_empty() {
        d.alias = d
            .repo
            .split('/')
            .nth(1)
            .unwrap()
            .chars()
            .take(64)
            .map(|c| if c == '.' { '-' } else { c })
            .collect();
    }
    {
        let _guard = app.gate.lock().await;
        app.lib.reserve(&d.alias, &d.repo, &d.preset)?;
    }
    tokio::spawn(async move {
        let _worker = app.worker.lock().await;
        app.log.lock().await.clear();
        let r = async {
            app.lib.finish(&d.alias, "downloading", "")?;
            fetch(&app, &d).await
        }
        .await;
        let update = match r {
            Ok(()) => app.lib.finish(&d.alias, "installed", ""),
            Err(e) => {
                let log = app.log.lock().await;
                app.lib.finish(
                    &d.alias,
                    "failed",
                    &format!("{e:#}\n{}", String::from_utf8_lossy(&log)),
                )
            }
        };
        if let Err(e) = update {
            eprintln!("Could not record download outcome: {e:#}");
        }
    });
    Ok(StatusCode::ACCEPTED)
}
async fn fetch(app: &App, d: &Download) -> Result<()> {
    #[derive(Deserialize)]
    struct Revision {
        sha: String,
    }
    #[cfg(not(feature = "dev-ui"))]
    let Packages::LoomFetch(runtime) = &app.packages;
    #[cfg(feature = "dev-ui")]
    let runtime = match &app.packages {
        Packages::LoomFetch(runtime) => runtime,
        Packages::Fixtures => {
            return crate::dev::download(&app.lib, &d.alias, &d.repo, &app.log).await;
        }
    };
    let rev = app
        .http
        .get(format!("https://huggingface.co/api/models/{}", d.repo))
        .send()
        .await?
        .error_for_status()?
        .json::<Revision>()
        .await?
        .sha;
    ensure!(
        rev.len() == 40 && rev.bytes().all(|c| c.is_ascii_hexdigit()),
        "Expected an immutable Hugging Face revision"
    );
    app.lib.revision(&d.alias, &rev)?;
    let staging = app.lib.staging(&d.alias);
    let mut cmd = runtime.fetch_command();
    cmd.arg(format!("huggingface:{}@{rev}", d.repo))
        .args(["--no-input", "--materialize=copy", "--output"])
        .arg(&staging);
    if !d.preset.is_empty() {
        cmd.arg("--preset").arg(&d.preset);
    }
    let mut child = cmd
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Launch loom-fetch")?;
    let mut stderr = child.stderr.take().unwrap();
    let mut buf = [0; 2048];
    loop {
        let n = stderr.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let mut log = app.log.lock().await;
        log.extend_from_slice(&buf[..n]);
        let excess = log.len().saturating_sub(8192);
        log.drain(..excess);
    }
    ensure!(
        child.wait().await?.success(),
        "Loom could not fetch this package. Check the download error and the repository’s Loom compatibility."
    );
    ensure!(
        staging.join("model.json").is_file(),
        "Fetched package lacks model.json"
    );
    tokio::fs::rename(staging, app.lib.package(&d.alias)).await?;
    Ok(())
}
async fn apply(app: &App, a: &str, op: &str) -> Result<()> {
    let _guard = app.gate.lock().await;
    let m = app.model(a)?;
    let registered = app.engine.status().await?.contains_key(a);
    match op {
        "start" => {
            ensure!(
                m.state == "installed" && !registered,
                "Package must be installed and stopped"
            );
            app.engine
                .register(a, &app.lib.package(a), &m.preset)
                .await?;
            app.lib.desired(a, true)?;
        }
        "stop" => {
            if registered {
                app.engine.stop(a).await?;
            }
            app.lib.desired(a, false)?;
        }
        "delete" => {
            ensure!(
                !registered && !m.desired && !matches!(m.state.as_str(), "downloading" | "queued"),
                "Stop the model and wait for retirement or download completion first"
            );
            for path in [app.lib.staging(a), app.lib.package(a)] {
                if path.exists() {
                    tokio::fs::remove_dir_all(path).await?;
                }
            }
            app.lib.forget(a)?;
        }
        _ => bail!("Unknown action"),
    }
    Ok(())
}
#[derive(serde::Serialize, Deserialize)]
struct Turn {
    role: String,
    content: String,
}
#[derive(Deserialize)]
struct Chat {
    prompt: String,
    history: Vec<Turn>,
    tokens: u32,
}
async fn chat(
    State(app): State<App>,
    Path(a): Path<String>,
    Json(c): Json<Chat>,
) -> Ui<Json<serde_json::Value>> {
    chat_result(&app, &a, c).await.map_err(Into::into)
}
fn response_text(reply: &serde_json::Value) -> Result<String> {
    let choice = &reply["choices"][0];
    let text = match &choice["message"]["content"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect(),
        _ => String::new(),
    };
    if !text.trim().is_empty() {
        return Ok(text);
    }
    if let Some(refusal) = choice["message"]["refusal"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
    {
        return Ok(refusal.into());
    }
    if choice["finish_reason"] == "length" {
        bail!(
            "The model reached the token limit without returning answer text. Increase Token limit and retry."
        );
    }
    bail!(
        "Loom returned no displayable answer text (finish reason: {}).",
        choice["finish_reason"].as_str().unwrap_or("not reported")
    );
}
async fn chat_result(app: &App, a: &str, c: Chat) -> Result<Json<serde_json::Value>> {
    app.model(a)?;
    ensure!(
        app.engine.status().await?.get(a).is_some_and(|l| l.ready()),
        "Model is not loaded"
    );
    ensure!(
        (1..=4096).contains(&c.tokens) && !c.prompt.trim().is_empty() && c.prompt.len() <= 8192,
        "Invalid message or token limit"
    );
    let mut turns = c.history;
    ensure!(
        turns.len() < 40
            && turns
                .iter()
                .enumerate()
                .all(|(i, t)| t.role == if i % 2 == 0 { "user" } else { "assistant" })
            && turns.len().is_multiple_of(2),
        "Invalid or full conversation. Start a new chat."
    );
    turns.push(Turn {
        role: "user".into(),
        content: c.prompt,
    });
    let start = Instant::now();
    let reply = app
        .engine
        .chat(a, &serde_json::to_value(&turns)?, c.tokens)
        .await?;
    let sec = start.elapsed().as_secs_f64();
    turns.push(Turn {
        role: "assistant".into(),
        content: response_text(&reply)?,
    });
    let usage = reply["usage"]["completion_tokens"]
        .as_u64()
        .map(|n| {
            format!(
                "{n} output tokens · {:.1} output tok/s (end-to-end)",
                n as f64 / sec
            )
        })
        .unwrap_or_else(|| "Token count not reported".into());
    Ok(Json(
        serde_json::json!({"turns":turns,"metrics":format!("{sec:.2} s · {usage}")}),
    ))
}
async fn metrics(State(app): State<App>) -> Ui<String> {
    Ok(app.engine.metrics().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chat_extracts_visible_text_or_explains_empty_answers() {
        use serde_json::json;
        let extract = |message, reason| {
            response_text(&json!({"choices":[{"message":message,"finish_reason":reason}]}))
        };
        assert_eq!(
            extract(json!({"content":"hello"}), "stop").unwrap(),
            "hello"
        );
        assert_eq!(
            extract(
                json!({"content":[{"type":"text","text":"hello"},{"type":"text","text":" world"}]}),
                "stop"
            )
            .unwrap(),
            "hello world"
        );
        assert_eq!(
            extract(json!({"refusal":"Not available"}), "stop").unwrap(),
            "Not available"
        );
        assert!(
            extract(
                json!({"content":"  ","reasoning_content":"thinking"}),
                "length"
            )
            .unwrap_err()
            .to_string()
            .contains("token limit")
        );
        assert!(extract(json!({"content":null}), "stop").is_err());
    }
    #[test]
    fn paste_extracts_only_model_repository_ids() {
        for link in [
            "publisher/model",
            "https://huggingface.co/publisher/model/",
            "https://huggingface.co/publisher/model/blob/main/model.json?download=true",
        ] {
            assert_eq!(repository(link).unwrap(), "publisher/model");
        }
        for bad in [
            "https://example.com/org/repo",
            "https://huggingface.co.example/org/repo",
            "https://huggingface.co/datasets/org/data",
            "../file",
            "https://huggingface.co/org/%2fsecret",
            "https://huggingface.co/org/",
        ] {
            assert!(repository(bad).is_err(), "{bad}");
        }
    }
}
