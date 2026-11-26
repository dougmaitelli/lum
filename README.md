# Lum

Lum is a web manager for local hrx-loom chat models. One Rust service lets you download models from Hugging Face, chat with them in the browser, and serve them to OpenAI-compatible clients. Installed models share one private engine; you choose which ones to load or pin.

![Lum Chat in mock mode: Qwen3-8B responds to a coding prompt with a syntax-highlighted Python greeting function](screenshots/library.png)

## Open Chat

1. Follow the [installation guide](packaging/INSTALL.md) for Fedora 44 (RPM), Debian/Ubuntu (DEB), or a manual installation. Download the tested x86-64 packages, tarball and checksums from [GitHub Releases](https://github.com/dougmaitelli/lum/releases/latest). Lum installs a private, pinned Loom SDK on first start.
2. Open **http://127.0.0.1:9390/** on the server, or [configure trusted LAN access](packaging/INSTALL.md#configure-the-listener) to use your workstation’s browser. The admin UI defaults to loopback without login. For authenticated access, [enable OpenID Connect](packaging/INSTALL.md#optional-openid-connect-for-admin-access).
3. Click **+ Add model**. Search `loom-fun` packages or paste a Hugging Face model link from a trusted publisher. Choose a preset if prompted, then click **Download**. The sidebar download button shows queued and active jobs, lets you cancel either, and closes the popover when the queue empties.
4. Select the installed model and click **Load model** at the bottom of Chat. The message box appears when the model is ready. A green dot means *loaded*; a filled pin means *pinned*. The footer shows engine status and system-wide GPU VRAM usage when the driver reports it.

### Which models stay loaded?

`models.max_loaded` defaults to **1**. Pin a model to load it on startup and protect it from automatic unloading. When you load another model, Lum replaces an unpinned one first, even if a slot is free. If only pinned models are loaded, Lum uses a free slot or refuses the load when full. You can manually unload a pinned model with the eject icon without losing its pin. See the [configuration guide](packaging/INSTALL.md#configure-the-listener) to change the limit.

For an unloaded model, **… → Settings** lets you change its display label and context limit without changing its API model ID.

Chat keeps a separate conversation per model in the browser tab and shows elapsed time, completion tokens and end-to-end output tokens/s when Loom reports usage. Removing a model clears its conversation and draft; reinstalling under the same name starts fresh. Replies stream as they are generated. **Stop** interrupts the request and keeps any visible partial answer. Lum sends no browser reply budget; Loom’s model and context limits still apply. `/metrics` exposes native Prometheus text directly.

## Connect an OpenAI-compatible client

Load a model, then open **API keys** in the sidebar. Copy the client base URL (normally `http://127.0.0.1:9391/v1`), create a named key, and copy the secret before leaving the page. Set the client’s model to the loaded alias. The separate inference listener supports `GET /v1/models` and `POST /v1/chat/completions` for text messages, including `stream: true` with optional `stream_options: {"include_usage": true}`. Both require `Authorization: Bearer <key>`. Revoke a key on the same page to reject subsequent requests; no restart is needed.

To serve another machine, configure [the inference listener](packaging/INSTALL.md#configure-the-listener), not the admin listener. Keys protect inference only. Without OIDC, anyone who can reach the admin listener can create and revoke keys; with OIDC, only allowed administrators can.

## Development

```sh
npm ci --prefix web
cargo dev
```

Open **http://127.0.0.1:9399/** for Chat with a mock runtime. Svelte, TypeScript, and CSS changes update in the browser without restarting the backend. Node.js is a build/development dependency only. See the [development guide](docs/development.md) for prerequisites, layout, checks and packaging.
