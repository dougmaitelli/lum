use crate::{
    library::Library,
    native::Native,
    ui::{self, App},
};
use anyhow::{Context, Result, bail, ensure};
use rustix::process::{Pid, Signal, kill_process};
use std::{env, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    net::TcpListener,
    process::{Child, Command},
    signal::unix::{SignalKind, signal},
    time::{sleep, timeout},
};

fn directory(name: &str) -> Result<PathBuf> {
    let dir = env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("{name} is missing; start lum.service with systemd"))?;
    ensure!(dir.is_absolute(), "{name} must be absolute");
    Ok(dir)
}
#[tokio::main(flavor = "current_thread")]
pub async fn run() -> Result<()> {
    let runtime = directory("RUNTIME_DIRECTORY")?;
    let lib = Library::open(&directory("STATE_DIRECTORY")?)?;
    let bin = PathBuf::from(
        env::var_os("LUM_RUNTIME_BIN").unwrap_or_else(|| "/opt/lum/runtime/bin".into()),
    );
    ensure!(bin.is_absolute(), "LUM_RUNTIME_BIN must be absolute");
    let port: u16 = env::var("LUM_ADMIN_PORT")
        .unwrap_or_else(|_| "9390".into())
        .parse()
        .context("Invalid LUM_ADMIN_PORT")?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .context("bind loopback admin listener")?;
    let port = listener.local_addr()?.port();
    let socket = runtime.join("engine.sock");
    let engine = Native::new(&socket)?;
    let app = App::new(lib.clone(), engine.clone(), bin.join("loom-fetch"))?;
    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut child = Command::new(bin.join("loom-serve"))
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
        .context("launch loom-serve")?;
    let run = async {
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
        eprintln!("Lum library: http://127.0.0.1:{port}/");
        axum::serve(listener, ui::router(app, port))
            .await
            .context("admin listener")
    };
    let result = tokio::select! {r=child.wait()=>{bail!("loom-serve exited: {}",r?)},r=run=>r,_=term.recv()=>Ok(()),_=interrupt.recv()=>Ok(())};
    stop(&mut child).await?;
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
