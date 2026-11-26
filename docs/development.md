# Developing Lum

Build on Linux with stable Rust, a C compiler (for bundled SQLite), and Node.js 24+ with npm (CI uses Node 24). Node builds the UI; it is **not** installed or invoked by the distributed application.

## Work on Chat

```sh
npm ci --prefix web  # first checkout, or after the frontend lockfile changes
cargo dev
```

Open **http://127.0.0.1:9399/**. Rust starts an isolated mock Loom engine and Vite, which serves Chat and proxies admin requests to an ephemeral loopback Rust listener. Host and same-origin write checks remain in effect. Edit a `.svelte`, `.ts` or `.css` file under `web/src/`: Vite updates the open page without restarting Rust. Model state and inference keys stay intact. Chat history and drafts are per model, in the browser tab. Restart the command for **Rust** changes. Ctrl-C stops both processes and discards the temporary library and dev keys.

`LUM_DEV_PORT` sets the Vite port; `LUM_DEV_INFERENCE_PORT` sets the separate mock bearer API port (default `9400`). Catalog search is live. Downloads, chat and engine metrics are simulated. No Loom installation, weights, GPU, system configuration or sudo required.

## Layout

| Path | Responsibility |
| --- | --- |
| `src/`, `tests/` | Rust service, admin and inference APIs, persistence, runtime adapter, integration tests |
| `web/src/App.svelte`, `web/src/pages/` | Runtime bootstrap and Svelte pages for Chat, API management, removal confirmation and setup |
| `web/src/components/` | Conversation/composer and Add model dialog |
| `web/src/api/` | Typed browser client for the private admin API, not the inference listener |
| `web/src/style.css` | UI styles; Prettier owns presentation-source formatting |
| `e2e/` | Independent Playwright package that exercises the embedded UI against the mock runtime |
| `packaging/`, `.github/workflows/` | Linux distribution, validation and tag-only GitHub Releases |

Rust serves the same Vite bundle for `/`, `/chat/{alias}` and `/api`. Svelte checks `/admin/runtime` before mounting the requested screen and offers retry on failed setup. `/admin/state` returns model, transfer and system snapshots; `/admin/chat/{alias}`, `/admin/models/{alias}/{op}` and `/admin/downloads` accept same-origin JSON writes. `/admin/api` lists endpoint and key metadata; key creation and revocation are writes under `/admin/keys`. The plaintext secret is returned only on creation. Removal requires confirmation in the UI and is revalidated by the backend. No page is rendered from Rust templates. Do not expose `/admin` via the bearer-protected `/v1` listener.

### Admin authentication boundary

`src/oidc.rs` wraps the admin routes before the public login page and assets are added. The outer Host/Origin middleware runs even for the callback and logout. The inference router never receives the session store. `src/oidc/tests.rs` uses an ephemeral signing key and token endpoint to exercise PKCE and verified ID-token claims without a network provider. Never log tokens, cookie values, or callback query strings. Discovery uses HTTPS and refuses redirects in production.

Dependency review: `openidconnect` 4.1 pulls in `rsa` 0.9 ([RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)) and `chrono` ([RUSTSEC-2026-0543](https://rustsec.org/advisories/RUSTSEC-2026-0543.html), unmaintained). Lum uses public keys to verify ID tokens and performs no RSA private-key signing or decryption. The repository's Rust advisory checks have not been suppressed; revisit the dependency chain before release.

## Validate

```sh
make fmt        # write rustfmt and Prettier changes
make check      # format checks, ESLint, Svelte/TS diagnostics, web build, Clippy and Rust tests
npm ci --prefix e2e
(cd e2e && npx --no-install playwright install chromium && npm test)
```

For native CPU acceptance tests, add a Loom 0.3.0 `loom-serve` to `PATH` and run `cargo test --locked --all-features -- --include-ignored`. Playwright launches the Rust binary with disposable fixtures and embedded assets; it does not rely on a running Vite server, network downloads or system service.

## Build and run with real Loom

`cargo run --locked -- run` starts the foreground manager and provisions its managed SDK. The default backend is `amdgpu`; use `LUM_DEVICE=task` for CPU. Configuration is separate from dev mode. Use `lum config --user ...` and restart to change the foreground listeners. See the [installation guide](../packaging/INSTALL.md) for OS prerequisites and system configuration.

Cargo's build script runs Vite into Cargo's `OUT_DIR` and embeds the generated HTML, CSS and JS. `cargo build --locked --release && packaging/build.sh` produces the Linux packages and checksums with nFPM 2.49.0. The server only needs the packaged Rust binary and normal OS/runtime prerequisites, not the source tree or Node modules.
