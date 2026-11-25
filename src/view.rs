use crate::{library::Model, native::Live};
use maud::{DOCTYPE, Markup, html};
use std::collections::BTreeMap;

pub fn page(title: &str, content: Markup) -> Markup {
    html! {
    (DOCTYPE) html lang="en" {head {meta charset="utf-8";meta name="viewport" content="width=device-width, initial-scale=1";title {(title) " · Lum"}link rel="stylesheet" href="/style.css";}
    body {header {.bar {a.brand href="/" {span.mark aria-hidden="true" {"▥"} "lum"}nav aria-label="Main" {a href="/" {"Library"}a href="/catalog" {"Catalog"}a href="/metrics" {"Metrics ↗"}}span.private {"LOCAL INSTANCE"}}}
    main {(content)}footer {"Lum / Local model manager"}}}}
}
pub fn error(msg: &str) -> Markup {
    page(
        "Request failed",
        html! {p.kicker {"REQUEST FAILED"}h1 {"Couldn't complete that action"}pre {(msg)}a.btn href="/" {"Return to library"}},
    )
}
fn action(a: &str, op: &str, label: &str) -> Markup {
    html! {form.inline method="post" action=(format!("/models/{a}/{op}")) {button type="submit" {(label)}}}
}
pub fn library(models: &[Model], live: &BTreeMap<String, Live>, log: &str) -> Markup {
    page(
        "Model library",
        html! {
         p.kicker {"WORKSPACE / OVERVIEW"}h1 {"Model library"}p.lede {"Manage the packages on this server and the models serving requests."}
         .stats {@for (label,num) in [("Installed",models.iter().filter(|m|m.state=="installed").count()),("Ready to chat",live.values().filter(|m|m.ready()).count()),("Downloading",models.iter().filter(|m|m.state=="downloading").count())] {div {small {(label)} strong {(num)}}}}
         section { .heading {div {p.kicker {"ON THIS SERVER"}h2 {"Your models"}}a href="/" {"Refresh ↻"}}
         @if models.is_empty() {.empty {strong {"No models yet"}p {"Download a Loom chat package to get started. Weights stay on disk until you start the model."}a.btn href="/catalog" {"Browse catalog →"}}}
         @for m in models { @let a=&m.alias; @let active=live.get(a); @let state=active.map_or(m.state.as_str(),|v|if v.ready(){"ready"}else{&v.registration});
         article {.heading {div {h3 {(a)} small {(m.repo) " · " (if m.preset.is_empty(){"default preset"}else{&m.preset}) @if !m.revision.is_empty(){" · " code {(&m.revision[..m.revision.len().min(12)])}}}}span.badge data-state=(state) {(state)}}
         @if m.desired {small {"Selected for startup"}}
         @if !m.error.is_empty() {p.warning {(&m.error)}} @if let Some(e)=active.and_then(|v|v.error.as_ref()){p.warning {(e.get("message").and_then(|v|v.as_str()).unwrap_or("Model preparation failed."))}}
         .tools {@if active.is_some_and(|v|v.ready()){a.btn href=(format!("/play/{a}")) {"Open playground →"}}
         @if active.is_some() || m.desired {(action(a,"stop","Stop"))} @else if m.state=="installed" {(action(a,"start","Start model"))}
         @if active.is_none() && !m.desired && m.state!="downloading" {a.subtle href=(format!("/remove/{a}")) {"Remove…"}}}}
         }
         @if !log.is_empty() {details {summary {"Latest download output"}pre {(log)}}}
         }
         section {p.kicker {"HUGGING FACE"}h2 {"Download a package"}p {"Use a trusted loom-fun chat package, not a standalone GGUF. Choose its preset from the publisher’s model card."}(download_form(""))}
        },
    )
}
pub fn download_form(repo: &str) -> Markup {
    html! {form.grid method="post" action="/download" {label {"Repository" input name="repo" value=(repo) placeholder="loom-fun/unsloth-Qwen3-0.6B-loom" required;}
    label {"Local alias" input name="alias" maxlength="64" placeholder="chat-small" required;}label {"Preset (optional)" input name="preset" maxlength="64" placeholder="e.g. chat";}button.primary type="submit" {"Download"}}}
}
pub fn catalog(ids: &[String]) -> Markup {
    page(
        "Package catalog",
        html! {p.kicker {"DISCOVERY / HUGGING FACE"}h1 {"Package catalog"}p.lede {"Published by loom-fun. Check the model card for text support, device requirements and available presets."}
        section {@for id in ids {article {.heading {strong {(id)}a.btn href=(format!("/package/{}",id.trim_start_matches("loom-fun/"))) {"Choose →"}}}} @if ids.is_empty(){p {"No packages found."}}}},
    )
}
pub fn package(repo: &str) -> Markup {
    page(
        "Install package",
        html! {p.kicker {"CATALOG / INSTALL"}h1 {"Add to your library"}section {h2 {(repo)}a href=(format!("https://huggingface.co/{repo}")) rel="noreferrer" {"Read model card ↗"}(download_form(repo))}},
    )
}
pub fn confirm(a: &str) -> Markup {
    page(
        "Remove model",
        html! {p.kicker {"LIBRARY / STORAGE"}h1 {"Remove " (a) "?"}section {p {"This deletes the local installation and library entry. Loom’s shared download cache is not cleared."}.tools {(action(a,"delete","Remove files"))a href="/" {"Cancel"}}}},
    )
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Turn {
    pub role: String,
    pub content: String,
}
pub fn playground(a: &str, turns: &[Turn], metrics: Option<&str>, limit: u32) -> Markup {
    page(
        "Playground",
        html! {p.kicker {"INFERENCE / PLAYGROUND"}h1 {(a)}p.lede {"Test this model with a local conversation. Responses appear when generation finishes."}
        section {.heading {h2 {"Conversation"}a href=(format!("/play/{a}")) {"New chat"}} @if turns.is_empty(){p.empty {"Send a message to test the model."}}
        @for turn in turns {article {p.kicker {(&turn.role)} pre {(&turn.content)}}} @if let Some(metrics)=metrics {p.badge {(metrics)}}
        form method="post" action=(format!("/play/{a}")) {input type="hidden" name="history" value=(serde_json::to_string(turns).unwrap());label {"Message" textarea name="prompt" rows="3" maxlength="8192" required {}}.tools {label {"Output token limit" input type="number" name="tokens" min="1" max="4096" value=(limit);}button.primary {"Send message"}}}}},
    )
}
