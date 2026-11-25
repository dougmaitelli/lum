use anyhow::{Result, bail};
use reqwest::{Client, Method};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};

#[derive(Clone)]
pub struct Native(Client);
#[derive(Deserialize)]
pub struct Live {
    pub registration: String,
    pub status: Option<Gate>,
    pub error: Option<Value>,
}
#[derive(Deserialize)]
pub struct Gate {
    pub admission: String,
}
impl Live {
    pub fn ready(&self) -> bool {
        self.registration == "ready" && self.status.as_ref().is_some_and(|s| s.admission == "open")
    }
}
impl Native {
    pub fn new(socket: &Path) -> Result<Self> {
        Ok(Self(
            Client::builder()
                .no_proxy()
                .unix_socket(socket.to_owned())
                .timeout(Duration::from_secs(180))
                .build()?,
        ))
    }
    async fn call(&self, path: &str, payload: Option<Value>) -> Result<reqwest::Response> {
        let mut req = self.0.request(
            if payload.is_some() {
                Method::POST
            } else {
                Method::GET
            },
            format!("http://localhost{path}"),
        );
        if let Some(doc) = payload {
            req = req.json(&doc)
        }
        let r = req.send().await?;
        if !r.status().is_success() {
            bail!("Loom {}: {}", r.status(), r.text().await?)
        }
        Ok(r)
    }
    pub async fn ready(&self) -> Result<()> {
        self.call("/readyz", None).await?;
        Ok(())
    }
    pub async fn status(&self) -> Result<BTreeMap<String, Live>> {
        #[derive(Deserialize)]
        struct Entry {
            id: String,
            loom: Live,
        }
        #[derive(Deserialize)]
        struct Status {
            data: Vec<Entry>,
        }
        Ok(self
            .call("/v1/status", None)
            .await?
            .json::<Status>()
            .await?
            .data
            .into_iter()
            .map(|e| (e.id, e.loom))
            .collect())
    }
    pub async fn register(&self, a: &str, dir: &Path, preset: &str) -> Result<()> {
        let mut model = json!({"path":"model.json"});
        if !preset.is_empty() {
            model["preset"] = json!(preset);
        }
        self.call(
            "/v1/models/register",
            Some(json!({"name":a,"base_directory":dir,"model":model,"definition":{"kind":"text"}})),
        )
        .await?;
        Ok(())
    }
    pub async fn stop(&self, a: &str) -> Result<()> {
        self.call(
            "/v1/models/unregister",
            Some(json!({"model":a,"mode":"drain"})),
        )
        .await?;
        Ok(())
    }
    pub async fn chat(&self, a: &str, messages: &Value, tokens: u32) -> Result<Value> {
        Ok(self
            .call(
                "/v1/chat/completions",
                Some(json!({"model":a,"messages":messages,"max_tokens":tokens,"stream":false})),
            )
            .await?
            .json()
            .await?)
    }
    pub async fn metrics(&self) -> Result<String> {
        Ok(self.call("/metrics", None).await?.text().await?)
    }
}
