use crate::{
    library::Library,
    native::Native,
    runtime::Runtime,
    ui::{self, App, Setup},
};
use anyhow::{Context, Result, bail, ensure};
use rustix::process::{Pid, Signal, kill_process};
use std::{
    env,
    fs::{DirBuilder, File, OpenOptions},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::TcpListener,
    process::{Child, Command},
    signal::unix::{SignalKind, signal},
    sync::{Notify, watch},
    time::{sleep, timeout},
};

fn directory(name: &str) -> Result<PathBuf> {
    let dir = env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("{name} is missing; start lum.service or use `lum run`"))?;
    ensure!(dir.is_absolute(), "{name} must be absolute");
    Ok(dir)
}
fn private_dir(dir: &Path) -> Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    Ok(())
}
fn instance_lock(state: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.join("instance.lock"))?;
    file.try_lock()
        .context("Another Lum process owns this state directory")?;
    Ok(file)
}
fn directories(foreground: bool) -> Result<(PathBuf, PathBuf)> {
    if !foreground {
        return Ok((
            directory("RUNTIME_DIRECTORY")?,
            directory("STATE_DIRECTORY")?,
        ));
    }
    let data = match env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => directory("HOME")?.join(".local/share"),
    };
    ensure!(data.is_absolute(), "XDG_DATA_HOME must be absolute");
    let state = data.join("lum");
    let runtime = match env::var_os("XDG_RUNTIME_DIR").filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("lum"),
        None => state.join("run"),
    };
    ensure!(runtime.is_absolute(), "XDG_RUNTIME_DIR must be absolute");
    private_dir(&state)?;
    private_dir(&runtime)?;
    Ok((runtime, state))
}

#[tokio::main(flavor = "current_thread")]
pub async fn run(foreground: bool) -> Result<()> {
    let (runtime, state) = directories(foreground)?;
    private_dir(&state)?;
    let _lock = instance_lock(&state)?;
    let mut settings = crate::config::Listener::load(foreground)?;
    let addr = std::net::SocketAddr::new(settings.host, settings.port);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Bind Lum listener at {addr}"))?;
    let addr = listener.local_addr()?;
    settings.port = addr.port();
    let browser_host = settings
        .name
        .clone()
        .unwrap_or_else(|| match settings.host {
            std::net::IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".into(),
            std::net::IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".into(),
            std::net::IpAddr::V6(ip) => format!("[{ip}]"),
            ip => ip.to_string(),
        });
    let url = format!("http://{browser_host}:{}/", settings.port);
    eprintln!("Lum listener bound to {addr}");
    let lib = Library::open(&state)?;
    let sdk = Runtime::select(&state)?;
    let socket = runtime.join("engine.sock");
    let engine = Native::new(&socket)?;
    let app = App::new(lib.clone(), engine.clone(), sdk.bin().join("loom-fetch"))?;
    let (phase, rx) = watch::channel(Setup::Preparing);
    let retry = Arc::new(Notify::new());
    let router = ui::router(app, settings, rx, retry.clone(), sdk.description())
        .into_make_service_with_connect_info::<ui::Destination>();
    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    eprintln!("Lum setup: {url}");
    let mut child = None;
    let manager = async {
        loop {
            phase.send_replace(Setup::Preparing);
            match sdk.ensure().await {
                Ok(()) => break,
                Err(e) => {
                    eprintln!("Runtime setup failed: {e:#}");
                    phase.send_replace(Setup::Failed(format!("{e:#}")));
                    retry.notified().await;
                }
            }
        }
        child = Some(
            Command::new(sdk.bin().join("loom-serve"))
                .args([
                    format!(
                        "--device={}",
                        env::var("LUM_DEVICE").unwrap_or_else(|_| "amdgpu".into())
                    ),
                    format!(
                        "--pool_backing={}",
                        env::var("LUM_POOL_BACKING").unwrap_or_else(|_| "fixed".into())
                    ),
                    format!("--listen=unix:{}", socket.display()),
                ])
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .context("launch loom-serve")?,
        );
        let wait = child.as_mut().unwrap().wait();
        let ready = async {
            timeout(Duration::from_secs(20), async {
                while engine.ready().await.is_err() {
                    sleep(Duration::from_millis(80)).await;
                }
            })
            .await
            .context("Engine did not become ready")?;
            for m in lib
                .list()?
                .into_iter()
                .filter(|m| m.desired && m.state == "installed")
            {
                if let Err(e) = engine
                    .register(&m.alias, &lib.package(&m.alias), &m.preset)
                    .await
                {
                    lib.finish(&m.alias, "installed", &format!("Restore failed: {e:#}"))?;
                }
            }
            phase.send_replace(Setup::Ready);
            eprintln!("Lum library: {url}");
            std::future::pending::<Result<()>>().await
        };
        tokio::select! {s=wait=>bail!("loom-serve exited: {}",s?),r=ready=>r}
    };
    let result = tokio::select! {r=manager=>r,r=axum::serve(listener,router)=>r.context("admin listener"),_=term.recv()=>Ok(()),_=interrupt.recv()=>Ok(())};
    if let Some(mut child) = child {
        stop(&mut child).await?;
    }
    result
}
async fn stop(child: &mut Child) -> Result<()> {
    if let Some(pid) = child.id().and_then(|n| Pid::from_raw(n as i32)) {
        let _ = kill_process(pid, Signal::TERM);
    }
    match timeout(Duration::from_secs(5), child.wait()).await {
        Ok(s) => {
            s?;
        }
        Err(_) => child.kill().await?,
    }
    Ok(())
}
