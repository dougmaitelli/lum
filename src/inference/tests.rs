use super::*;
use crate::{native::Native, runtime::Runtime};
use anyhow::Result;
use std::sync::{Arc, Mutex};

#[test]
fn malformed_tool_requests_and_non_text_parts_are_rejected() {
    let base = json!({"messages":[{"role":"user","content":"hello"}]});
    for (key, val) in [
        ("parallel_tool_calls", json!("false")),
        ("tool_choice", json!("sometimes")),
        ("tools", json!([{"type":"function","function":{}}])),
        (
            "messages",
            json!([{"role":"user","content":[{"type":"image_url","image_url":{"url":"http://example.org"}}]}]),
        ),
        ("messages", json!([{"role":"tool","content":"22"}])),
        (
            "messages",
            json!([{"role":"assistant","tool_calls":[{"type":"function","id":"1","function":{"name":"read","arguments":{}}}]}]),
        ),
    ] {
        let mut body = base.clone();
        body[key] = val;
        assert!(tools::validate(&body).is_err(), "{key}");
    }
}

// Exercise the public bearer-protected HTTP listener against a deterministic
// upstream. No actual Home Assistant tools or devices are invoked.
#[tokio::test]
async fn home_assistant_function_call_round_trip() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let socket = tmp.path().join("loom.sock");
    let private = tokio::net::UnixListener::bind(&socket)?;
    let tool = json!({"type":"function","function":{"name":"ReadTemperature","description":"Read temperature","parameters":{"type":"object","properties":{}}}});
    let call = json!({"id":"call_1","type":"function","function":{"name":"ReadTemperature","arguments":"{}"}});
    let first = json!({"model":"test","messages":[{"role":"system","content":"You are a home assistant"},{"role":"user","content":[{"type":"text","text":"What is the temperature?"}]}],"tools":[tool],"parallel_tool_calls":true,"stream":true,"stream_options":{"include_usage":true}});
    let mut followup = first.clone();
    let messages = followup["messages"].as_array_mut().unwrap();
    messages.push(json!({"role":"assistant","content":null,"tool_calls":[call]}));
    messages.push(json!({"role":"tool","tool_call_id":"call_1","content":"22 C"}));
    followup["tool_choice"] = json!("none");
    let mut delta = call;
    delta["index"] = json!(0);
    let chunk = |delta: Value, reason: Value| {
        json!({"choices":[{"index":0,"delta":delta,"finish_reason":reason}]}).to_string()
    };
    let calls = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        chunk(
            json!({"role":"assistant","tool_calls":[delta]}),
            Value::Null
        ),
        chunk(json!({}), json!("tool_calls"))
    );
    let answer = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        chunk(json!({"content":"22 C"}), json!("stop"))
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let responses = [calls.clone(), answer.clone()];
    let upstream=Router::new().route("/v1/status",get(||async{Json(json!({"data":[{"id":"test","loom":{"registration":"ready","status":{"admission":"open"}}}]}))})).route("/v1/chat/completions",post(move|Json(body):Json<Value>|{let mut v=captured.lock().unwrap();let response=responses[v.len()].clone();v.push(body);async move{([(header::CONTENT_TYPE,"text/event-stream")],response)}}));
    tokio::spawn(async move { axum::serve(private, upstream).await.unwrap() });
    let lib = Library::open(tmp.path())?;
    lib.reserve("test", "org/test", "")?;
    lib.finish("test", "installed", "")?;
    let key = lib.create_key("fixture")?;
    let app = App::new(
        lib,
        Native::new(&socket)?,
        Runtime::External(tmp.path().into()),
    )?;
    let (_phase, rx) = tokio::sync::watch::channel(Setup::Ready);
    let api = router(app, 1, rx);
    let public = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = public.local_addr()?;
    tokio::spawn(async move { axum::serve(public, api).await.unwrap() });
    let http = reqwest::Client::builder().no_proxy().build()?;
    for (body, expected) in [(&first, calls), (&followup, answer)] {
        let response = http
            .post(format!("http://{addr}/v1/chat/completions"))
            .bearer_auth(&key)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        assert_eq!(response.text().await?, expected);
    }
    assert_eq!(*seen.lock().unwrap(), [first, followup]);
    Ok(())
}
