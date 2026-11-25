# Lum architecture

**Decision:** Lum is a Rust application for managing local Loom chat models on a Linux server. Our application code, including CLI, HTTP handlers, UI behavior and integration tests, is Rust. HTML/CSS are presentation assets. `hrx-loom[hf]` is an external execution engine. Lum does not import its Python modules or need Python to build. The Rust manager owns installation into isolated version-specific environments, invokes verified tools as child processes, and can optionally use an explicit service-readable external SDK instead. Initial integration target: hrx-loom 0.3.0.

## Process and network boundaries

```text
Browser (localhost or SSH tunnel) ── admin listener ─┐
                                                   ├── lum (Tokio + Axum) ── SQLite + package directory
Local services ── inference listener, bearer key ───┘       │
                                                          ├── loom-fetch (finite download job)
                                                          └── loom-serve (one private multi-model engine)
```

Use one Cargo package and one managed Linux service, not one service per model. Add modules as features land; don't split this into independently deployed services. The inference listener exposes an explicit allowlist (`GET /v1/models`, `POST /v1/chat/completions`), never a catch-all proxy. Both listeners default to loopback. Binding inference to the LAN is explicit; use TLS or a trusted tunnel on untrusted networks. Do not forward admin credentials or native control headers. The native engine and telemetry remain private.

One engine reduces scheduling and memory contention between independent runtimes on the shared GPU. The tradeoff is a shared failure domain: an engine crash ends all in-flight requests. The supervisor must report that failure, restart with bounded backoff, and restore desired registrations; never silently replay a chat request.

**Current deployment stage:** A single boot-enabled unit runs the Rust manager, which owns a loopback-by-default Axum UI (explicit LAN binding trusts all reachable clients; no web credentials), SQLite library and private `loom-serve` child. Start/Stop persist desired registrations; startup restores selected aliases. An unexpected child exit fails the manager so systemd can restart the unit with rate limiting. A second inference-only listener requires a bearer key on both routes, including on loopback. The admin UI issues random 256-bit keys and stores their SHA-256 digests in SQLite. Every new inference request checks the key store; no bearer or client headers reach the engine. Both browser chat and streaming API completions relay Loom’s SSE incrementally. Aborting the browser request drops its upstream response rather than leaving a detached task consuming generation. See [installation and service operations](packaging/INSTALL.md).

## State ownership

| Owner | Authoritative facts |
|---|---|
| SQLite (Lum) | Stable alias, trusted repo and immutable revision, preset, local installation path, restore-on-boot intent, download outcome. |
| Package directory | Downloaded files. Fetch into staging and publish only after verification succeeds. Interrupted downloads must never appear installed. |
| Loom | Live registration, readiness, admission, memory residency and runtime counters. A successful registration request is **not** a ready model. |

Download does not imply start. Starting binds an installed package to an alias without restarting the engine. Stopping drains and unregisters only that alias. Reconcile desired registrations against observed state on startup; surface individual failures without taking healthy aliases offline. Never delete an installation still in use. Do not automatically evict another running model to start a new one.

## Integration and UI

Keep command-line arguments and native HTTP payloads in a Loom adapter. Invoke binaries with argument vectors, not a shell. Search lists `loom-fun` text packages; operators may also choose another trusted publisher via a Hugging Face model repository link. Only validated repository IDs reach the Hugging Face API and Loom fetcher, never arbitrary download URLs. A standalone GGUF is not a Loom package. Use streaming with backpressure and cancellation; do not buffer a completion before returning it to the caller. Browser chat and API clients use the same private engine; only API requests require inference keys. Label time to first text and end-to-end output tokens/s separately from engine decode throughput. Use reported token counts; if unavailable, show unavailable.

The entire admin UI is a Svelte + TypeScript client. It reads snapshots and submits actions over the same-origin admin JSON API; polling updates component state, not HTML fragments. Setup status and retry remain accessible while the engine is unavailable; a Rust readiness gate protects the operational API. All screens load from the same embedded frontend bundle. In development Vite provides HMR and proxies admin requests without rewriting Host or Origin. Production uses no frontend server: Cargo builds and embeds the Vite output alongside the supported SDK wheel lock. A first system boot provisions `/opt/lum/runtimes/<version>/` as the unprivileged `lum-svc` identity. The parent `/opt/lum` is root-owned; only the runtime store is writable there. Service state stays at `/var/lib/lum`, which need not allow execution. Foreground instances retain their per-user layout and may set `LUM_RUNTIME_ROOT` to a dedicated executable directory. In both cases only successful tool probes publish the completion marker. Model storage is independent. Future UI switching should select from curated, hash-locked versions: install the candidate alongside the working version, drain requests, restart and probe the candidate, restore registrations, and roll back on failure. No web-triggered root package upgrades or arbitrary binary URLs. Updating the **Lum** executable remains an OS-package-manager operation.

## Delivery and acceptance

1. Runtime discovery (`lum doctor`); then supervise one private engine and exercise readiness/shutdown using CPU fixtures.
2. Add persistent installs, download jobs, registration and restore; test interrupted downloads, failed preparation and restart recovery.
3. Add the library, chat UI and inference-only listener. Test streaming, client disconnects and rejection of admin operations on the LAN port.
4. On the target Strix Halo, run **two real packages concurrently**. Record memory before/after each start and first inference, first-text latency and output throughput alone and under contention. Stop one alias while the other streams. Restart the service and verify only selected aliases return. CPU fixtures validate lifecycle, not GPU capacity or performance.
