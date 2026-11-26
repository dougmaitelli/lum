//! Public inference surface: two explicit routes, never a proxy to Loom administration.
#[cfg(test)]
mod tests;
mod tools;
use crate::{
    library::{Library, slug},
    ui::{App, LoadDenied, Setup, readiness, request_load},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

#[derive(Clone)]
struct Api {
    app: App,
    max: usize,
    phase: tokio::sync::watch::Receiver<Setup>,
}
pub fn router(app: App, max: usize, phase: tokio::sync::watch::Receiver<Setup>) -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .route_layer(middleware::from_fn_with_state(app.lib.clone(), authorize))
        .with_state(Api { app, max, phase })
}
fn error(status: StatusCode, msg: &str, kind: &str) -> Response {
    (
        status,
        Json(json!({"error":{"message":msg,"type":kind,"code":null,"param":null}})),
    )
        .into_response()
}
async fn authorize(State(lib): State<Library>, req: Request, next: Next) -> Response {
    let mut headers = req.headers().get_all(header::AUTHORIZATION).iter();
    let token = headers
        .next()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .filter(|(kind, _)| kind.eq_ignore_ascii_case("Bearer") && headers.next().is_none())
        .map_or("", |(_, t)| t);
    let authorized = if token.len() == 47 && token.starts_with("lum_") {
        let token = token.to_owned();
        lib.run(move |lib| lib.permits(&token)).await
    } else {
        Ok(false)
    };
    let mut response = match authorized {
        Ok(true) => next.run(req).await,
        Ok(false) => {
            let mut r = error(
                StatusCode::UNAUTHORIZED,
                "Missing or invalid API key",
                "authentication_error",
            );
            r.headers_mut()
                .insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
            r
        }
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Key store unavailable",
            "server_error",
        ),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
async fn models(State(api): State<Api>) -> Response {
    match api.app.lib.run(Library::list).await {
        Ok(installs) => {
            let data: Vec<_> = installs
                .into_iter()
                .filter(|m| m.state == "installed")
                .map(|m| json!({"id":m.alias,"object":"model","owned_by":"lum","created":0}))
                .collect();
            Json(json!({"object":"list","data":data})).into_response()
        }
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Library unavailable",
            "server_error",
        ),
    }
}
async fn prepare(api: &Api, alias: &str) -> anyhow::Result<crate::ui::LoadState> {
    anyhow::ensure!(
        matches!(*api.phase.borrow(), Setup::Ready),
        "Runtime not ready"
    );
    let mut load = request_load(&api.app, alias, api.max).await?;
    if load.ready {
        return Ok(load);
    }
    tokio::time::timeout(std::time::Duration::from_secs(300), async {
        loop {
            let live = api.app.engine.status().await?;
            let model = live.get(alias).ok_or(LoadDenied::Retiring)?;
            if readiness(model)? {
                load.ready = true;
                return Ok(load);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    })
    .await?
}
async fn chat(State(api): State<Api>, body: Result<Json<Value>, JsonRejection>) -> Response {
    let body = match body {
        Ok(Json(v)) => v,
        Err(e) => {
            return error(
                e.status(),
                "Expected a JSON chat completion request (maximum 1 MiB)",
                "invalid_request_error",
            );
        }
    };
    let invalid = |msg: &str| error(StatusCode::BAD_REQUEST, msg, "invalid_request_error");
    if !body["model"].as_str().is_some_and(slug) {
        return invalid("Use an installed model alias");
    }
    if let Err(msg) = tools::validate(&body) {
        return invalid(msg);
    }
    if body.get("stream").is_some_and(|v| !v.is_boolean()) {
        return invalid("stream must be a boolean");
    }
    if let Some(opts) = body.get("stream_options")
        && !(body["stream"] == true
            && opts.as_object().is_some_and(|o| {
                o.iter()
                    .all(|(k, v)| k == "include_usage" && v.is_boolean())
            }))
    {
        return invalid(
            "stream_options requires streaming and accepts only a boolean include_usage",
        );
    }
    if let Some(opts) = body.get("chat_template_kwargs")
        && !opts
            .as_object()
            .is_some_and(|o| o.len() == 1 && opts["enable_thinking"].is_boolean())
    {
        return invalid("chat_template_kwargs accepts only a boolean enable_thinking");
    }
    let allowed = [
        "chat_template_kwargs",
        "model",
        "messages",
        "stream",
        "stream_options",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "max_tokens",
        "temperature",
        "top_p",
        "stop",
        "seed",
    ];
    if let Some(key) = body
        .as_object()
        .unwrap()
        .keys()
        .find(|k| !allowed.contains(&k.as_str()))
    {
        return invalid(&format!("Unsupported chat completion option: {key}"));
    }
    let streaming = body["stream"] == true;
    let load = match prepare(&api, body["model"].as_str().unwrap()).await {
        Ok(load) => load,
        Err(e) => {
            return match e.downcast_ref::<LoadDenied>() {
                Some(reason) => {
                    let (status, kind) = match reason {
                        LoadDenied::NotInstalled => (StatusCode::NOT_FOUND, "model_not_found"),
                        LoadDenied::Full(_) | LoadDenied::Retiring => {
                            (StatusCode::CONFLICT, "model_unavailable")
                        }
                        LoadDenied::Failed => (StatusCode::SERVICE_UNAVAILABLE, "model_load_error"),
                    };
                    error(status, &reason.to_string(), kind)
                }
                None => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Model not ready. Check the model status in Lum and retry.",
                    "model_load_error",
                ),
            };
        }
    };
    let response = match api.app.engine.completion(body, load.disable_thinking).await {
        Ok(r) => r,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Engine unavailable",
                "server_error",
            );
        }
    };
    if !response.status().is_success() {
        return error(
            response.status(),
            "Engine rejected the completion. Check the loaded model and request options.",
            "inference_error",
        );
    }
    let mime = if streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    if !response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.split(';').next() == Some(mime))
    {
        return error(
            StatusCode::BAD_GATEWAY,
            "Unexpected engine response format",
            "server_error",
        );
    }
    (
        [(header::CONTENT_TYPE, mime)],
        crate::native::relay_body(response),
    )
        .into_response()
}
