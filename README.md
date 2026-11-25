# Lum

Lum is a Rust web manager for local hrx-loom chat models. Use one system service to browse `loom-fun` packages on Hugging Face, download a pinned revision, and run multiple models on one private engine. The library distinguishes installed packages from models ready for requests, and restores started aliases after a restart.

![Lum Chat in dev mode, with downloaded models in the sidebar](screenshots/library.png)

## Open Chat

1. Follow the [installation guide](packaging/INSTALL.md) for Fedora 44 (RPM), Debian/Ubuntu (DEB), or a manual installation. Download the tested x86-64 packages, tarball and checksums from [GitHub Releases](https://github.com/dougmaitelli/lum/releases/latest). Lum installs a private, pinned Loom SDK on first start.
2. On the server, open **http://127.0.0.1:9390/**. From a workstation, first run `ssh -N -L 9390:127.0.0.1:9390 user@server`, then open the same URL. The UI has no login and defaults to loopback. Use `lum config` to [enable access on a trusted LAN](packaging/INSTALL.md#configure-the-listener).
3. Click **+ Add model** in the sidebar. Search the live `loom-fun` catalog or paste a Hugging Face model link from a trusted publisher. Choose a preset if prompted. Click **Download**; **Downloads** shows queued and active jobs, with a progress bar for the current transfer. Finished jobs disappear; installation errors are available under **+ Add model**. Choose an installed model from the sidebar and click **Load model**. A green dot marks a loaded model. The bottom status bar shows engine availability, the loaded count, and driver-reported VRAM used / total and percentage. VRAM is system-wide per GPU (not Lum-only); on AMD unified-memory systems it describes the VRAM pool, not all shared RAM. Missing counters are shown as unavailable.

Chat keeps a separate conversation per model in the browser tab and shows elapsed time, completion tokens and end-to-end output tokens/s when Loom reports usage. Replies appear after generation finishes. `/metrics` exposes native Prometheus text directly. The chat UI does not yet stream replies or enforce GPU memory admission limits.

## Connect an OpenAI-compatible client

Load a model, then open **API keys** in the sidebar. Copy the client base URL (normally `http://127.0.0.1:9391/v1`), create a named key, and copy the secret before leaving the page. Set the client’s model to the loaded alias. The separate inference listener supports `GET /v1/models` and `POST /v1/chat/completions` for text messages, including `stream: true`. Both require `Authorization: Bearer <key>`. Revoke a key on the same page to reject subsequent requests; no restart is needed.

To serve another machine, configure [the inference listener](packaging/INSTALL.md#configure-the-listener), not the password-free admin listener. Keys protect inference only. Keep the admin UI restricted: anyone who can reach it can create and revoke keys.

## Development

```sh
npm ci --prefix web
cargo dev
```

Open **http://127.0.0.1:9399/** for Chat with a mock runtime. Svelte, TypeScript, and CSS changes update in the browser without restarting the backend. Node.js is a build/development dependency only. See the [development guide](docs/development.md) for prerequisites, layout, checks and packaging.
