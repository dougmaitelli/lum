use super::{App, Ui, repository};
use crate::library::slug;
use anyhow::{Context, Result, ensure};
use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub(super) struct Lookup {
    repo: String,
}

pub(super) async fn list(State(app): State<App>, Query(q): Query<Lookup>) -> Ui<Json<Vec<String>>> {
    let repo = repository(&q.repo)?;
    let root = format!("https://huggingface.co/{repo}/resolve/HEAD");
    let response = app
        .http
        .get(format!("{root}/model.json"))
        .send()
        .await
        .context("Fetch model card")?;
    let mut names = if response.status() == reqwest::StatusCode::NOT_FOUND {
        let catalog = app
            .http
            .get(format!("{root}/catalog.json"))
            .send()
            .await
            .context("Fetch package catalog")?;
        #[derive(Deserialize)]
        struct Entry {
            presets: Vec<String>,
        }
        #[derive(Deserialize)]
        struct Catalog {
            models: BTreeMap<String, Entry>,
        }
        let mut models = metadata::<Catalog>(catalog).await?.models;
        ensure_single(&models)?;
        models.pop_first().unwrap().1.presets
    } else {
        #[derive(Deserialize)]
        struct Card {
            #[serde(default)]
            presets: BTreeMap<String, serde::de::IgnoredAny>,
        }
        metadata::<Card>(response)
            .await?
            .presets
            .into_keys()
            .collect()
    };
    ensure_names(&names)?;
    names.sort();
    names.dedup();
    Ok(Json(names))
}
fn ensure_single<T>(models: &BTreeMap<String, T>) -> Result<()> {
    ensure!(
        models.len() == 1,
        "Expected one model in the package catalog"
    );
    Ok(())
}
fn ensure_names(names: &[String]) -> Result<()> {
    ensure!(
        names.iter().all(|s| slug(s)),
        "Package contains unsupported preset names"
    );
    Ok(())
}
async fn metadata<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let mut response = response.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 2 * 1024 * 1024,
            "Package metadata exceeds 2 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
