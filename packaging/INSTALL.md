# Install Lum on a Linux server

Use **the RPM on Fedora 44** and **the DEB on Ubuntu 24.04+ or Debian 13**. The third option is a manual tarball. These x86-64 builds are for systemd hosts with glibc 2.39 or newer. Choose one installation method; do not overlay a manual install on package-managed files.

The service starts the Lum web library on **127.0.0.1:9390** and one private Loom engine on a Unix socket. No LAN API or public listener is enabled. Lum runs as an unprivileged service account; neither login nor `loginctl enable-linger` is needed for boot startup.

## 1. Install the external runtime once

Lum is Rust; Python is needed here only for the separately distributed Loom SDK. Use Python 3.12 or newer. On Fedora install `python3` and `python3-pip` with `dnf`; on Debian/Ubuntu install `python3` and `python3-venv` with `apt`. Create the **root-owned, system-readable** environment in the service's expected location (not in a user's home):

```sh
sudo install -d -m 0755 /opt/lum
sudo python3 -m venv /opt/lum/runtime
sudo /opt/lum/runtime/bin/python -m pip install 'hrx-loom[hf]==0.3.0'
```

On your Strix Halo, check `/opt/lum/runtime/bin/amdf-info --format=json` and `getent group render video`. The unit grants these supplementary groups for device access. The host kernel/driver and device permissions must support Loom. For CPU-only hosts see below. The native runtime and model weights are not bundled or downloaded by a package maintainer script.

## 2. Install Lum

Download **lum-linux-x86_64** from the Artifacts section of a successful GitHub Actions **Build Linux distribution** run. Extract the artifact ZIP, change into its directory, and run `sha256sum -c SHA256SUMS`. Use artifacts from a commit you trust; these are CI builds, not signed distro-repository packages.

### Fedora 44

```sh
sudo dnf install ./lum*.rpm
```

### Ubuntu / Debian

```sh
sudo apt install ./lum*.deb
```

Both packages install `/usr/bin/lum`, `/usr/lib/systemd/system/lum.service`, `/etc/lum/lum.env`, and this guide. On first install they enable `lum.service` for boot and start it if systemd is running and the external runtime is present. Debian service-start policy is respected. When installing into an image/chroot, enablement is recorded on disk. If the runtime is installed later, run `sudo systemctl start lum.service`.

### Manual (no package manager)

```sh
tar -xzf lum-*-linux-x86_64.tar.gz
cd lum-*-linux-x86_64
sudo install -Dm755 lum /usr/bin/lum
sudo install -Dm644 lum.service /usr/lib/systemd/system/lum.service
sudo install -Dm644 INSTALL.md /usr/share/doc/lum/INSTALL.md
# Keep local configuration on subsequent installations.
if ! test -f /etc/lum/lum.env; then sudo install -Dm644 lum.env /etc/lum/lum.env; fi
sudo systemctl daemon-reload
sudo systemctl enable --now lum.service
```

For a manual upgrade, repeat the file installation, reload units, and `sudo systemctl restart lum.service` to use the new binary. To install manually **from source**, run `cargo build --locked --release`, then use `target/release/lum`, `packaging/lum.service`, `packaging/lum.env` and `packaging/INSTALL.md` as the four source files in the same `install` commands.

## 3. Verify and configure

```sh
PATH="/opt/lum/runtime/bin:$PATH" lum doctor
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

If the unit reports a failed condition, check that `/opt/lum/runtime/bin/loom-serve` exists and is executable. For GPU errors inspect service logs and device permissions; on Fedora also check audit logs for SELinux denials instead of disabling SELinux. After correcting repeated startup failures, run `sudo systemctl reset-failed lum && sudo systemctl start lum`.

## Open the web UI

On the server, visit `http://127.0.0.1:9390/`. From another computer, run `ssh -N -L 9390:127.0.0.1:9390 user@server`, then open that same URL locally. If you change `LUM_ADMIN_PORT`, use the same port at both ends. The admin interface trusts local users; do not expose it through a public reverse proxy.

1. Open **Catalog**, choose a trusted `loom-fun` text package, and check the model card for device, memory and preset requirements. For example, `loom-fun/unsloth-Qwen3-0.6B-loom` has a `chat` preset. Choose a stable alias and click **Download**. Catalog discovery and downloading need outbound internet access.
2. Click **Refresh** in the library for download progress, latest output and registration state. Downloading is separate from starting; one download runs at a time. A failed/interrupted download can be removed and retried.
3. Click **Start model**, refresh until ready, then **Open playground**. The playground holds history in the browser form and returns a complete reply with elapsed time and reported output tokens/s (including prompt time). **Metrics** shows the engine’s raw Prometheus counters. There is no streaming playground or LAN inference listener yet.
4. **Stop** gracefully retires that alias. After retirement, **Remove…** deletes its local package; it does not clear the SDK cache. Allow disk space for both the cache and installed weights.

## Removal

Use `sudo dnf remove lum` or `sudo apt purge lum`. For a manual install:

```sh
sudo systemctl disable --now lum.service
sudo rm -f /usr/bin/lum /usr/lib/systemd/system/lum.service /usr/share/doc/lum/INSTALL.md
sudo systemctl daemon-reload
```

Runtime files under `/opt/lum/runtime`, local overrides, and persistent state are not deleted automatically. Back up state before intentionally removing it. Do not rely on a dynamic service UID for manual ownership of files outside systemd-managed directories.
