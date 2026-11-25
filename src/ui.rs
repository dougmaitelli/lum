use crate::{
    library::{Library, Model, slug, valid_repo},
    native::Native,
    view::{self, Turn},
};
use anyhow::{Context, Result, bail, ensure};
use axum::{
    Form, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use maud::Markup;
use reqwest::Client;
use serde::Deserialize;
use std::{
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, process::Command, sync::Mutex};

#[derive(Clone)]
pub struct App {
    pub lib: Library,
    pub engine: Native,
    http: Client,
    fetch: PathBuf,
    gate: Arc<Mutex<()>>,
    log: Arc<Mutex<Vec<u8>>>,
}
impl App {
    pub fn new(lib: Library, engine: Native, fetch: PathBuf) -> Result<Self> {
        Ok(Self {
            lib,
            engine,
            fetch,
            http: Client::builder()
                .user_agent("Lum local model library")
                .timeout(Duration::from_secs(25))
                .build()?,
            gate: Default::default(),
            log: Default::default(),
        })
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
        (
            StatusCode::BAD_REQUEST,
            view::error(&format!("{:#}", self.0)),
        )
            .into_response()
    }
}
type Ui<T> = std::result::Result<T, Problem>;

pub fn router(app: App, port: u16) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/catalog", get(catalog))
        .route("/package/{name}", get(package))
        .route("/download", post(download))
        .route("/models/{alias}/{op}", post(change))
        .route("/remove/{alias}", get(confirm))
        .route("/play/{alias}", get(play).post(chat))
        .route("/metrics", get(metrics))
        .route(
            "/style.css",
            get(async || {
                (
                    [(header::CONTENT_TYPE, "text/css")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .layer(DefaultBodyLimit::max(256 * 1024))
        .layer(middleware::from_fn(move |req, next| {
            protect(req, next, port)
        }))
        .with_state(app)
}

async fn protect(req: Request<Body>, next: Next, port: u16) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let local = host == format!("localhost:{port}") || host == format!("127.0.0.1:{port}");
    let read_only = matches!(*req.method(), Method::GET | Method::HEAD);
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok());
    if !local || (!read_only && origin != Some(format!("http://{host}").as_str())) {
        return (
            StatusCode::FORBIDDEN,
            "Use the loopback admin URL and same-origin form.",
        )
            .into_response();
    }
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY,"default-src 'none'; style-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'".parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
}
async fn home(State(app): State<App>) -> Ui<Markup> {
    let models = app.lib.list()?;
    let live = app.engine.status().await?;
    let log = app.log.lock().await;
    Ok(view::library(
        &models,
        &live,
        &String::from_utf8_lossy(&log),
    ))
}
async fn catalog(State(app): State<App>) -> Ui<Markup> {
    #[derive(Deserialize)]
    struct Item {
        id: String,
    }
    let result = app
        .http
        .get("https://huggingface.co/api/models?author=loom-fun&limit=500")
        .send()
        .await
        .context("Catalog unavailable; you can enter a repository on the library page")?
        .error_for_status()
        .context("Hugging Face catalog request failed")?
        .json::<Vec<Item>>()
        .await
        .context("Invalid catalog response")?;
    let mut ids: Vec<_> = result
        .into_iter()
        .map(|i| i.id)
        .filter(|s| valid_repo(s))
        .collect();
    ids.sort();
    Ok(view::catalog(&ids))
}
async fn package(Path(name): Path<String>) -> Ui<Markup> {
    let repo = format!("loom-fun/{name}");
    if !valid_repo(&repo) {
        return Err(Problem(anyhow::anyhow!("Invalid repository")));
    }
    Ok(view::package(&repo))
}
async fn confirm(State(app): State<App>, Path(a): Path<String>) -> Ui<Markup> {
    app.model(&a)?;
    Ok(view::confirm(&a))
}
#[derive(Deserialize)]
struct Download {
    alias: String,
    repo: String,
    preset: String,
}
async fn download(State(app): State<App>, Form(d): Form<Download>) -> Ui<Redirect> {
    {
        let _guard = app.gate.lock().await;
        app.lib.reserve(&d.alias, &d.repo, &d.preset)?;
        app.log.lock().await.clear();
    }
    tokio::spawn(async move {
        let r = fetch(&app, &d).await;
        let update = match r {
            Ok(()) => app.lib.finish(&d.alias, "installed", ""),
            Err(e) => app.lib.finish(&d.alias, "failed", &format!("{e:#}")),
        };
        if let Err(e) = update {
            eprintln!("Could not record download outcome: {e:#}");
        }
    });
    Ok(Redirect::to("/"))
}
async fn fetch(app: &App, d: &Download) -> Result<()> {
    #[derive(Deserialize)]
    struct Revision {
        sha: String,
    }
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
    let mut cmd = Command::new(&app.fetch);
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
        "Fetch failed. Check latest download output, remove the failed entry, and retry."
    );
    ensure!(
        staging.join("model.json").is_file(),
        "Fetched package lacks model.json"
    );
    tokio::fs::rename(staging, app.lib.package(&d.alias)).await?;
    Ok(())
}
async fn change(State(app): State<App>, Path((a, op)): Path<(String, String)>) -> Ui<Redirect> {
    apply(&app, &a, &op).await?;
    Ok(Redirect::to("/"))
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
                !registered && !m.desired && m.state != "downloading",
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
async fn play(State(app): State<App>, Path(a): Path<String>) -> Ui<Markup> {
    app.model(&a)?;
    Ok(view::playground(&a, &[], None, 128))
}
#[derive(Deserialize)]
struct Chat {
    prompt: String,
    history: String,
    tokens: u32,
}
async fn chat(State(app): State<App>, Path(a): Path<String>, Form(c): Form<Chat>) -> Ui<Markup> {
    chat_result(&app, &a, c).await.map_err(Into::into)
}
async fn chat_result(app: &App, a: &str, c: Chat) -> Result<Markup> {
    app.model(a)?;
    ensure!(
        app.engine.status().await?.get(a).is_some_and(|l| l.ready()),
        "Model is not ready"
    );
    ensure!(
        (1..=4096).contains(&c.tokens) && !c.prompt.trim().is_empty() && c.prompt.len() <= 8192,
        "Invalid message or token limit"
    );
    let mut turns: Vec<Turn> = serde_json::from_str(&c.history)?;
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
    let content = reply["choices"][0]["message"]["content"]
        .as_str()
        .context("No text in Loom response")?;
    turns.push(Turn {
        role: "assistant".into(),
        content: content.into(),
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
    Ok(view::playground(
        a,
        &turns,
        Some(&format!("{sec:.2} s · {usage}")),
        c.tokens,
    ))
}
async fn metrics(State(app): State<App>) -> Ui<String> {
    Ok(app.engine.metrics().await?)
}
