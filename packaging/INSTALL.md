# Install Lum on a Linux server

Use **the RPM on Fedora 44** and **the DEB on Ubuntu 24.04+ or Debian 13**. The third option is a manual tarball. These x86-64 builds are for systemd hosts with glibc 2.39 or newer. Choose one installation method; do not overlay a manual install on package-managed files.

The service starts the Lum web library on **127.0.0.1:9390** and one private Loom engine on a Unix socket. No LAN API or public listener is enabled by default. Lum runs as an unprivileged service account; neither login nor `loginctl enable-linger` is needed for boot startup.

## 1. Runtime setup is automatic

Do not install Loom or uv for Lum. The RPM/DEB installs the OS Python prerequisites; on a manual tarball installation supply `/usr/bin/python3` (3.12+) with venv and ensurepip (`python3-venv` on Debian/Ubuntu; `python3` and `python-pip-wheel` on Fedora). On first start Lum downloads the hash-locked hrx-loom 0.3.0 wheels and Hugging Face dependencies from PyPI as the unprivileged service account. The system service provisions executable SDKs under `/opt/lum/runtimes/<version>/` as `lum-svc`; models and SQLite stay under `/var/lib/lum` and may be on a `noexec` mount. On upgrade a previously installed or partial SDK under `/var/lib/lum/runtimes` is ignored, not moved (venvs contain absolute paths). Lum reinstalls the verified SDK once at the new path without deleting or migrating the model library. The library URL shows setup status and allows retry on failure. Subsequent starts reuse the verified runtime without network access. Model downloads are separate. For restricted networks an administrator may instead specify `LUM_RUNTIME_BIN=/absolute/service-readable/bin` in `lum.env`; Lum never modifies an external SDK. No personal uv install is searched or changed.

**Just testing?** After extracting the updated manual tarball, run `./lum run` as your normal user and open the printed URL. No systemd installation is needed. It uses `$XDG_DATA_HOME/lum` (default `~/.local/share/lum`) for the library and versioned runtimes, and `$XDG_RUNTIME_DIR/lum` for the private engine socket when available. Stop with Ctrl-C. To test without an AMD GPU use `LUM_DEVICE=task ./lum run`.

On Strix Halo the host AMD driver, device permissions and `render`/`video` groups still need to work. Lum manages userspace Loom, not host GPU drivers.

## 2. Install Lum

Open [Lum releases](https://github.com/dougmaitelli/lum/releases) and download the **RPM**, **DEB**, or **linux-x86_64.tar.gz** from the release’s Assets, plus `SHA256SUMS` from the same release. In that directory run `sha256sum --check --ignore-missing SHA256SUMS` and confirm your downloaded package is listed as `OK`. These are tested release builds, not signed distro-repository packages. Untagged development builds remain available as GitHub Actions artifacts.

### Fedora 44

```sh
sudo dnf install ./lum*.rpm
```

### Ubuntu / Debian

```sh
sudo apt install ./lum*.deb
```

Both packages install `/usr/bin/lum`, `/usr/lib/systemd/system/lum.service`, `/etc/lum/lum.env`, and this guide. On first install they enable `lum.service` for boot and start it if systemd is running. Debian service-start policy is respected. When installing into an image/chroot, enablement is recorded on disk. After upgrading from an older release that was skipped for unmet conditions, run `sudo systemctl enable --now lum.service`.

### Manual (no package manager)

```sh
tar -xzf lum-*-linux-x86_64.tar.gz
cd lum-*-linux-x86_64
sudo install -Dm755 lum /usr/bin/lum
sudo install -Dm644 lum.service /usr/lib/systemd/system/lum.service
sudo install -Dm644 INSTALL.md /usr/share/doc/lum/INSTALL.md
sudo install -Dm644 sysusers.d/lum.conf /usr/lib/sysusers.d/lum.conf
sudo install -Dm644 tmpfiles.d/lum.conf /usr/lib/tmpfiles.d/lum.conf
sudo systemd-sysusers lum.conf
sudo systemd-tmpfiles --create /usr/lib/tmpfiles.d/lum.conf
# Keep local configuration on subsequent installations.
if ! test -f /etc/lum/lum.env; then sudo install -Dm644 lum.env /etc/lum/lum.env; fi
sudo systemctl daemon-reload
sudo systemctl enable --now lum.service
```

For a manual upgrade, repeat the file installation, reload units, and `sudo systemctl restart lum.service` to use the new binary. To install manually **from source**, install the [build prerequisites](../docs/development.md), run `npm ci --prefix web` and `cargo build --locked --release`, then use `target/release/lum` for the binary and the corresponding files under `packaging/` for all other sources in the same commands.

## 3. Verify and configure

```sh
systemctl is-enabled lum.service       # enabled: will start on subsequent boots
systemctl status lum.service --no-pager
sudo curl --fail --unix-socket /run/lum/engine.sock http://localhost/readyz
journalctl -u lum.service -b --no-pager
```

`/readyz` reports engine readiness even without a model. The socket is intentionally private. `systemctl status` alone does not verify engine readiness. The unit recreates `/run/lum` at start; `/var/lib/lum` is persistent. `Restart=on-failure` recovers a crashed process with rate limiting. The library and pinned package revisions live under `/var/lib/lum`. Start marks a model for restoration on subsequent boots; Stop clears that selection. Failed preparations are reported in the library.

Edit `/etc/lum/lum.env` for runtime settings, then `sudo systemctl restart lum.service`. For CPU-only validation set `LUM_DEVICE=task`. If `render` or `video` groups are absent on a CPU machine, use `sudo systemctl edit lum.service` and clear the list:

```ini
[Service]
SupplementaryGroups=
```

If the unit reports an unmet condition for `/opt/lum`, inspect `systemctl cat lum.service`: the old unit or a local override is still loaded. The updated unit does not have a Loom-path prerequisite. For runtime setup errors open the UI or read `journalctl -u lum -b`; after fixing Python, disk space or network access use **Retry setup**. For GPU errors inspect service logs and device permissions; on Fedora also check audit logs for SELinux denials instead of disabling SELinux. After correcting repeated startup failures, run `sudo systemctl reset-failed lum && sudo systemctl start lum`.

## Configure the listener

The system service reads `/etc/lum/config.json` on startup. Read settings as a normal user; use `sudo` to change them and restart the service. For a server named `ai`:

```sh
lum config
sudo lum config api.host 0.0.0.0
sudo lum config api.name ai          # optional: allow http://ai:9390 as well as the server IP
sudo systemctl restart lum.service
```

Open `http://<server-ip>:9390/` or `http://ai:9390/` if that name resolves. **There is no web login: anyone who can reach that port can manage models and create or revoke inference keys.** Keep loopback (the default) unless the listener is restricted to trusted clients by your firewall or VPN. Host and browser origin checks remain in effect; they are not authentication. Lum does not open your firewall.

`lum config` prints the effective settings grouped by prefix. For example:

```text
api:
  host: 0.0.0.0
  port: 9390
  name: ai
inference:
  host: 127.0.0.1
  port: 9391
models:
  max_loaded: 1
```

`lum config api.host` reads one value; `sudo lum config api.port 9494` sets the port. To return to local-only access use `sudo lum config api.host 127.0.0.1` and `sudo lum config api.name -`. Restart after changes. The `api.*` settings control only the **admin HTTP listener**.

By default one model may be loaded at a time. To allow two, run `sudo lum config models.max_loaded 2` and `sudo systemctl restart lum.service`. Loading beyond the limit is rejected; Lum never evicts a running model to make room. When restarting with a lower limit, Lum restores previously loaded models in alias order up to the limit and leaves the rest installed but unloaded. The limit does not restrict downloads or simultaneous requests to an already loaded model.

The **API keys** sidebar page shows the inference base URL to use in your client. Load a model, create a named key there, copy it once, and supply it to an OpenAI-compatible client as its API key. SQLite stores only the key digest and metadata. Revoking a key blocks new requests immediately; it does not cancel a request already underway.

To allow LAN clients while keeping administration local, leave `api.host` at loopback and set `sudo lum config inference.host <server-lan-ip>` (or `0.0.0.0` for all IPv4 interfaces). Optionally set `sudo lum config inference.port 9391`, then `sudo systemctl restart lum.service`. When using a wildcard bind, use the server’s reachable IP/hostname rather than `127.0.0.1` in remote clients. Expose only the inference port. HTTP does not encrypt bearer keys: use TLS at a reverse proxy or a secure tunnel on untrusted networks.

For `lum run`, use `lum config --user ...` (no sudo) and restart the foreground process. It reads `$XDG_CONFIG_HOME/lum/config.json` or `~/.config/lum/config.json`. An absolute `LUM_CONFIG_FILE` overrides the path for both the CLI and a process started with that variable. A saved `api.port` takes precedence over `LUM_ADMIN_PORT`; otherwise the environment or default `9390` applies. Existing runtime/device settings in `lum.env` continue to work.

## Open the web UI

On the server, visit `http://127.0.0.1:9390/`. From another computer, run `ssh -N -L 9390:127.0.0.1:9390 user@server`, then open that same URL locally. If you change the listener port, use the same port at both ends. The default loopback interface trusts local users; do not expose it through a public reverse proxy.

1. In Chat, click **+ Add model**. Search the live `loom-fun` catalog or paste a trusted publisher’s Hugging Face model link. Only Loom-compatible packages can run. Check device, memory and preset requirements on the model card. For example, `loom-fun/unsloth-Qwen3-0.6B-loom` has a `chat` preset. A single preset is selected automatically. If there are several, choose one from the dropdown. Selecting a package or preset does not start a download. Lum derives the local model name from the repository. Search and real downloads require outbound internet access.
2. Click **Download**. Open **Downloads** to see only active and queued jobs while you chat. One fetch runs at a time. Use **Cancel** on a queued or active job to release its reservation and discard its partial install without restarting Lum. The shared SDK download cache is kept. The progress bar shows the current file/stage percentage when Loom reports one and is indeterminate otherwise. The managed SDK reports ongoing work about twice per second when byte events are available; external `LUM_RUNTIME_BIN` tools control their own reporting cadence. Finished jobs disappear. If an install fails or is interrupted, open **+ Add model** for the error and remove the failed entry before retrying its alias.
3. Select an installed model from the sidebar and click **Load model**. When the green dot appears, send a message. Chat keeps history per model in the browser tab and streams each reply, with elapsed time and reported output tokens/s (including prompt time). `/metrics` serves the raw Prometheus counters. Use **Stop** to interrupt generation. Any visible reply is kept; if no text arrived, the prompt returns to the composer. Reloading during generation restores the prompt as a draft, not automatically resent. Partial replies that reach the model’s output limit are marked. API clients may request SSE streaming; the stream has an idle timeout rather than a total generation deadline.
The system service installs models in `/var/lib/lum/models/<alias>/`; during installation the staging path is `/var/lib/lum/models/.<alias>.partial`. A foreground `lum run` uses `$XDG_DATA_HOME/lum/models/` or, by default, `~/.local/share/lum/models/`. Loom’s shared fetch cache is separate (normally `/var/lib/lum/.cache/loom/` for the service).

For an unloaded model, choose **… → Settings** to edit its display label or context limit. The API model ID stays the same. Context covers prompt, history and reply; leave it blank for the preset default. Loom checks the requested capacity on load. Chat uses the preset’s output-token default rather than imposing a 128-token cap; API clients can still specify `max_tokens` per request.

4. The eject icon in the model’s sidebar row unloads it. Once unloaded, choose **… → Remove** and confirm to delete its local package; it does not clear the SDK cache. Allow disk space for both the cache and installed weights.

## Removal

Use `sudo dnf remove lum` or `sudo apt purge lum`. For a manual install:

```sh
sudo systemctl disable --now lum.service
sudo rm -f /usr/bin/lum /usr/lib/systemd/system/lum.service /usr/share/doc/lum/INSTALL.md \
  /usr/lib/sysusers.d/lum.conf /usr/lib/tmpfiles.d/lum.conf
sudo systemctl daemon-reload
```

The `lum-svc` system account, managed runtimes under `/opt/lum/runtimes/`, any older SDK directories, local overrides, and persistent state are not deleted automatically. Back up state before intentionally removing it. Do not rely on a dynamic service UID for manual ownership of files outside systemd-managed directories.
