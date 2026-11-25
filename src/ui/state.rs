use super::{App, Ui};
use crate::{library::Model, native::Live};
use axum::{Json, extract::State};
use serde::Serialize;
use std::{path::Path, time::Duration};

/// Loom's piped stderr reports byte percentages per file/stage, not per package.
#[derive(Default, serde::Serialize)]
pub struct FetchProgress {
    stage: String,
    percent: Option<u8>,
}
impl FetchProgress {
    pub fn from_log(log: &[u8]) -> Self {
        let text = String::from_utf8_lossy(log);
        let Some((complete, _)) = text.rsplit_once('\n') else {
            return Self::default();
        };
        for line in complete.lines().rev() {
            let mut words = line.split_whitespace();
            if words.next() != Some("loom-fetch") {
                continue;
            }
            let Some(
                stage @ ("Metadata" | "Output" | "Access" | "Fetch" | "Download" | "Transfer"
                | "Read" | "Verify" | "Transform" | "Install" | "Copy" | "Complete"),
            ) = words.next()
            else {
                continue;
            };
            let percent = words
                .next()
                .filter(|s| s.starts_with('[') && s.ends_with(']'))
                .and_then(|_| words.next()?.strip_suffix('%')?.parse().ok())
                .filter(|p| *p <= 100);
            return Self {
                stage: stage.into(),
                percent,
            };
        }
        Self::default()
    }
}

#[derive(Serialize)]
struct ModelState {
    #[serde(flatten)]
    model: Model,
    registered: bool,
    loaded: bool,
}
#[derive(Serialize)]
struct Gpu {
    card: String,
    summary: String,
}
#[derive(Serialize)]
pub(super) struct Snapshot {
    models: Vec<ModelState>,
    online: bool,
    mock: bool,
    vram: Vec<Gpu>,
    progress: FetchProgress,
}

pub(super) async fn status(State(app): State<App>) -> Ui<Json<Snapshot>> {
    let (status, gpus) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(3), app.engine.status()),
        tokio::task::spawn_blocking(|| crate::telemetry::read_vram(Path::new("/sys/class/drm")))
    );
    let registrations = status.ok().and_then(Result::ok);
    let models = app
        .lib
        .run(crate::library::Library::list)
        .await?
        .into_iter()
        .map(|mut model| {
            let live = registrations.as_ref().and_then(|l| l.get(&model.alias));
            if let Some(message) = live
                .and_then(|v| v.error.as_ref())
                .and_then(|e| e["message"].as_str())
            {
                model.error = message.to_owned();
            }
            ModelState {
                model,
                registered: live.is_some(),
                loaded: live.is_some_and(Live::ready),
            }
        })
        .collect();
    let vram = gpus
        .unwrap_or_default()
        .into_iter()
        .map(|gpu| Gpu {
            summary: gpu.summary(),
            card: gpu.card,
        })
        .collect();
    Ok(Json(Snapshot {
        models,
        vram,
        online: registrations.is_some(),
        mock: app.mocked(),
        progress: FetchProgress::from_log(&app.log.lock().await),
    }))
}
