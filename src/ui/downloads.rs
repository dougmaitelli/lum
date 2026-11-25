//! One fetch at a time. Queued jobs can be cancelled without waiting for the worker.
use super::{App, Download, Ui, fetch};
use crate::library::Library;
use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use std::{
    collections::HashMap,
    io::ErrorKind,
    sync::{Arc, Mutex},
};
use tokio::sync::{Mutex as AsyncMutex, oneshot};

#[derive(Clone, Default)]
pub(super) struct Queue {
    worker: Arc<AsyncMutex<()>>,
    jobs: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
}

pub(super) struct ProcessGroup(pub Option<rustix::process::Pid>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
}

pub(super) async fn enqueue(app: App, d: Download) -> Result<()> {
    // A disconnected POST must not leave a DB reservation without a worker.
    tokio::spawn(async move {
        let guard = app.gate.lock().await;
        let request = d.clone();
        app.lib
            .run(move |lib| lib.reserve(&request.alias, &request.repo, &request.preset))
            .await?;
        let (tx, rx) = oneshot::channel();
        app.downloads
            .jobs
            .lock()
            .unwrap()
            .insert(d.alias.clone(), tx);
        drop(guard);
        tokio::spawn(async move {
            if let Err(e) = work(&app, &d, rx).await {
                let msg = format!("{e:#}");
                if let Err(db) = app
                    .lib
                    .for_alias(&d.alias, move |lib, a| lib.finish(a, "failed", &msg))
                    .await
                {
                    eprintln!("Could not record download outcome: {db:#}");
                }
            }
        });
        Ok(())
    })
    .await?
}

pub(super) async fn cancel(State(app): State<App>, Path(alias): Path<String>) -> Ui<StatusCode> {
    let mut jobs = app.downloads.jobs.lock().unwrap();
    let tx = jobs
        .remove(&alias)
        .context("Download is no longer queued or fetching")?;
    tx.send(())
        .map_err(|_| anyhow::anyhow!("Download already finished"))?;
    Ok(StatusCode::ACCEPTED)
}

async fn work(app: &App, d: &Download, mut cancelled: oneshot::Receiver<()>) -> Result<()> {
    let slot = tokio::select! { biased; _ = &mut cancelled => None, slot = app.downloads.worker.lock() => Some(slot) };
    let result = if slot.is_some() {
        app.log.lock().await.clear();
        match app
            .lib
            .for_alias(&d.alias, |lib, a| lib.finish(a, "downloading", ""))
            .await
        {
            Ok(()) => {
                tokio::select! { biased; _ = &mut cancelled => None, r = fetch(app,d) => Some(r) }
            }
            Err(e) => Some(Err(e)),
        }
    } else {
        None
    };
    // Atomically stop accepting cancellations before promoting staging. A cancel
    // accepted just after fetch completed still wins; it never removes an install.
    let accepted_cancel = app
        .downloads
        .jobs
        .lock()
        .unwrap()
        .remove(&d.alias)
        .is_none();
    if let Some(result) = result.filter(|_| !accepted_cancel) {
        let rev = match result {
            Ok(rev) => rev,
            Err(e) => anyhow::bail!("{e:#}\n{}", String::from_utf8_lossy(&app.log.lock().await)),
        };
        app.lib
            .for_alias(&d.alias, move |lib, a| lib.revision(a, &rev))
            .await?;
        tokio::fs::rename(app.lib.staging(&d.alias), app.lib.package(&d.alias)).await?;
        app.lib
            .for_alias(&d.alias, |lib, a| lib.finish(a, "installed", ""))
            .await
    } else {
        match tokio::fs::remove_dir_all(app.lib.staging(&d.alias)).await {
            Ok(()) => (),
            Err(e) if e.kind() == ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        app.lib.for_alias(&d.alias, Library::forget).await
    }
}
