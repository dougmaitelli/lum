use anyhow::{Result, bail};
use reqwest::{Client, Method};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};

// The downstream body owns the upstream response: disconnecting the browser
// drops Loom's stream, rather than leaving a detached generation task running.
pub fn relay_body(response: reqwest::Response) -> axum::body::Body {
    axum::body::Body::from_stream(futures_util::stream::try_unfold(
        response,
        |mut r| async move { Ok::<_, reqwest::Error>(r.chunk().await?.map(|b| (b, r))) },
    ))
}

#[derive(Clone)]
pub struct Native {
    control: Client,
    generation: Client,
    stream: Client,
}
#[derive(Deserialize)]
pub struct Live {
    pub registration: String,
    pub status: Option<Gate>,
    pub error: Option<Value>,
}
#[derive(Deserialize)]
pub struct Gate {
    pub admission: String,
    pub phase: Option<String>,
}
impl Live {
    pub fn failed(&self) -> bool {
        matches!(self.registration.as_str(), "failed" | "release_failed")
            || self.status.as_ref().and_then(|s| s.phase.as_deref()) == Some("failed")
    }
    pub fn ready(&self) -> bool {
        !self.failed()
            && self.registration == "ready"
            && self.status.as_ref().is_some_and(|s| s.admission == "open")
    }
}
impl Native {
    pub fn new(socket: &Path) -> Result<Self> {
        Self::with_timeouts(socket, Duration::from_secs(30), Duration::from_secs(180))
    }
    fn with_timeouts(socket: &Path, control: Duration, idle: Duration) -> Result<Self> {
        let client = || {
            Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .unix_socket(socket.to_owned())
                .connect_timeout(Duration::from_secs(5))
        };
        // Non-streaming Loom calls send no heartbeat before the completed answer.
        Ok(Self {
            control: client().timeout(control).build()?,
            generation: client().build()?,
            stream: client().read_timeout(idle).build()?,
        })
    }
    async fn call(&self, path: &str, payload: Option<Value>) -> Result<reqwest::Response> {
        let mut req = self.control.request(
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
    pub async fn register(
        &self,
        a: &str,
        dir: &Path,
        preset: &str,
        context_limit: Option<i32>,
    ) -> Result<()> {
        let mut model = json!({"path":"model.json"});
        if !preset.is_empty() {
            model["preset"] = json!(preset);
        }
        let mut definition = json!({"kind":"text"});
        if let Some(limit) = context_limit {
            definition["context_capacity"] = json!(limit);
        }
        self.call(
            "/v1/models/register",
            Some(json!({"name":a,"base_directory":dir,"model":model,"definition":definition})),
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
    pub async fn completion(&self, payload: Value) -> Result<reqwest::Response> {
        let client = if payload["stream"] == true {
            &self.stream
        } else {
            &self.generation
        };
        Ok(client
            .post("http://localhost/v1/chat/completions")
            .json(&payload)
            .send()
            .await?)
    }
    pub async fn chat(&self, a: &str, messages: &Value) -> Result<Value> {
        let r = self
            .completion(json!({"model":a,"messages":messages,"stream":false}))
            .await?;
        if !r.status().is_success() {
            bail!("Loom {}: {}", r.status(), r.text().await?)
        }
        Ok(r.json().await?)
    }
    pub async fn metrics(&self) -> Result<String> {
        Ok(self.call("/metrics", None).await?.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        body::{Body, Bytes},
        response::IntoResponse,
        routing::post,
    };
    #[tokio::test]
    async fn inference_is_not_limited_by_total_duration() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("server.sock");
        let socket = tokio::net::UnixListener::bind(&path)?;
        let route = Router::new().route(
            "/v1/chat/completions",
            post(|Json(body): Json<Value>| async move {
                if body["stream"] == true {
                    let pieces = futures_util::stream::unfold(0, |n| async move {
                        if n == 5 {
                            return None;
                        }
                        tokio::time::sleep(Duration::from_millis(60)).await;
                        Some((
                            Ok::<_, std::io::Error>(Bytes::from_static(b"data\n")),
                            n + 1,
                        ))
                    });
                    Body::from_stream(pieces).into_response()
                } else {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    Json(json!({"ok":true})).into_response()
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(socket, route).await.unwrap();
        });
        let n =
            Native::with_timeouts(&path, Duration::from_millis(40), Duration::from_millis(200))?;
        assert_eq!(n.chat("model", &json!([])).await?["ok"], true);
        assert_eq!(
            n.completion(json!({"stream":true})).await?.text().await?,
            "data\n".repeat(5)
        );
        server.abort();
        Ok(())
    }
}
