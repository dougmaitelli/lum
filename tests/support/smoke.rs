use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use tokio::signal::unix::{SignalKind, signal};

use crate::engine::{Api, Engine};

async fn answer(api: &Api, alias: &str) -> Result<()> {
    let reply: Value = api.chat(alias, false).await?.json().await?;
    ensure!(
        reply["model"] == alias
            && reply["choices"][0]["message"]["content"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
        "no chat answer from {alias}: {reply}"
    );
    Ok(())
}

async fn exercise(api: &Api, package: &Path) -> Result<()> {
    api.wait_empty().await?;
    println!("Private CPU engine ready, with no registered models.");
    for alias in ["alpha", "beta"] {
        api.register(alias, package, "check").await?;
    }
    tokio::try_join!(answer(api, "alpha"), answer(api, "beta"))?;
    println!("Both aliases answered concurrent requests.");

    // Keep beta's client stream open across alpha's graceful retirement. With a
    // tiny fixture the device may already have produced tokens in socket buffers.
    let mut stream = api.chat("beta", true).await?;
    let first = stream.chunk().await?.context("empty beta stream")?;
    api.remove("alpha").await?;
    let mut data = first.to_vec();
    while let Some(chunk) = stream.chunk().await? {
        data.extend_from_slice(&chunk);
    }
    let mut text = String::new();
    let mut done = false;
    for line in std::str::from_utf8(&data)?
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
    {
        match line.trim() {
            "[DONE]" => done = true,
            json => {
                let event: Value = serde_json::from_str(json)?;
                ensure!(
                    event["model"] == "beta",
                    "unexpected beta stream event: {event}"
                );
                if let Some(delta) = event["choices"][0]["delta"]["content"].as_str() {
                    text.push_str(delta);
                }
            }
        }
    }
    ensure!(done && !text.is_empty(), "incomplete beta chat stream");
    answer(api, "beta").await?;
    println!("Alpha retired; beta's stream completed and a fresh beta chat succeeded.");
    Ok(())
}

pub async fn run(dir: &Path, binary: &Path, temp_root: &Path) -> Result<()> {
    let package = dir
        .canonicalize()
        .context("resolve CPU fixture directory")?;
    ensure!(
        package.join("model.json").is_file(),
        "missing fixture model.json"
    );
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let mut engine = Engine::spawn_cpu(binary, temp_root)?;
    let api = engine.api.clone();
    let result = tokio::select! {
        r = engine.run_until_exit(exercise(&api, &package)) => r,
        _ = interrupt.recv() => Err(anyhow::anyhow!("interrupted")),
        _ = terminate.recv() => Err(anyhow::anyhow!("terminated")),
    };
    let shutdown = engine.shutdown().await;
    if let Err(e) = shutdown {
        bail!("engine shutdown failed: {e:#}; lifecycle result: {result:?}");
    }
    result?;
    println!("Engine stopped and reaped.");
    Ok(())
}
