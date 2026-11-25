//! Public inference surface: two explicit routes, never a proxy to Loom administration.
use crate::{
    library::{Library, slug},
    native::Native,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::stream;
use serde_json::{Value, json};

pub fn router(lib: Library, engine: Native) -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .route_layer(middleware::from_fn_with_state(lib, authorize))
        .with_state(engine)
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
    let mut response = match lib.permits(token) {
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
async fn models(State(engine): State<Native>) -> Response {
    match engine.status().await {
        Ok(live) => {
            let data: Vec<_> = live
                .into_iter()
                .filter(|(_, v)| v.ready())
                .map(|(id, _)| json!({"id":id,"object":"model","owned_by":"lum","created":0}))
                .collect();
            Json(json!({"object":"list","data":data})).into_response()
        }
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Engine unavailable",
            "server_error",
        ),
    }
}
async fn chat(State(engine): State<Native>, body: Result<Json<Value>, JsonRejection>) -> Response {
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
    if !body["model"].as_str().is_some_and(slug)
        || !body["messages"].as_array().is_some_and(|m| {
            !m.is_empty()
                && m.iter().all(|v| {
                    matches!(v["role"].as_str(), Some("system" | "user" | "assistant"))
                        && v["content"].is_string()
                })
        })
    {
        return invalid(
            "Use a loaded model alias and text messages with system, user or assistant roles",
        );
    }
    if body.get("stream").is_some_and(|v| !v.is_boolean()) {
        return invalid("stream must be a boolean");
    }
    let allowed = [
        "model",
        "messages",
        "stream",
        "max_tokens",
        "temperature",
        "top_p",
        "stop",
        "seed",
    ];
    if body
        .as_object()
        .unwrap()
        .keys()
        .any(|k| !allowed.contains(&k.as_str()))
    {
        return invalid("Unsupported chat completion option");
    }
    let streaming = body["stream"] == true;
    let response = match engine.completion(body).await {
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
    let chunks = stream::try_unfold(response, |mut r| async move {
        Ok::<_, reqwest::Error>(r.chunk().await?.map(|b| (b, r)))
    });
    ([(header::CONTENT_TYPE, mime)], Body::from_stream(chunks)).into_response()
}
