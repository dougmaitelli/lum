//! Validate the supported text/function-calling shapes without rewriting the
//! request. Loom generates tool calls; only the API client executes them.
use serde_json::Value;

fn nonempty(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty())
}
fn text(v: &Value) -> bool {
    v.is_string()
        || v.as_array().is_some_and(|parts| {
            !parts.is_empty()
                && parts
                    .iter()
                    .all(|p| p["type"] == "text" && p["text"].is_string())
        })
}
fn function(v: &Value) -> bool {
    v["type"] == "function" && nonempty(&v["function"]["name"])
}
fn call(v: &Value) -> bool {
    function(v) && nonempty(&v["id"]) && v["function"]["arguments"].is_string()
}
fn message(v: &Value) -> bool {
    match v["role"].as_str() {
        Some("system" | "user") => text(&v["content"]) && v.get("tool_calls").is_none(),
        Some("assistant") => match v.get("tool_calls") {
            None => text(&v["content"]),
            Some(calls) => {
                calls
                    .as_array()
                    .is_some_and(|a| !a.is_empty() && a.iter().all(call))
                    && (v["content"].is_null() || text(&v["content"]))
            }
        },
        Some("tool") => nonempty(&v["tool_call_id"]) && text(&v["content"]),
        _ => false,
    }
}
pub(super) fn validate(body: &Value) -> Result<(), &'static str> {
    if !body["messages"]
        .as_array()
        .is_some_and(|m| !m.is_empty() && m.iter().all(message))
    {
        return Err(
            "Expected text messages, assistant function calls, or tool results with a tool_call_id",
        );
    }
    if body.get("tools").is_some_and(|v| {
        !v.as_array().is_some_and(|a| {
            a.iter().all(|t| {
                function(t) && t["function"].get("parameters").is_none_or(Value::is_object)
            })
        })
    }) {
        return Err(
            "tools must be an array of named function definitions with optional parameter schemas",
        );
    }
    if body
        .get("parallel_tool_calls")
        .is_some_and(|v| !v.is_boolean())
    {
        return Err("parallel_tool_calls must be a boolean");
    }
    if body
        .get("tool_choice")
        .is_some_and(|v| !matches!(v.as_str(), Some("none" | "auto" | "required")) && !function(v))
    {
        return Err("tool_choice must be none, auto, required, or a named function");
    }
    Ok(())
}
