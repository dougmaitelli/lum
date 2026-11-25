//! Lum-owned engine installations. Each supported release has its own venv and
//! embedded, hash-locked wheel manifest. Neither the host Python nor uv is modified.
use anyhow::{Context, Result, ensure};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, time::timeout};

pub const VERSION: &str = "0.3.0";
const LOCK: &str = include_str!("../runtimes/hrx-loom-0.3.0.lock");

pub enum Runtime {
    Managed(PathBuf),
    External(PathBuf),
}
impl Runtime {
    pub fn select(state: &Path) -> Result<Self> {
        if let Some(path) = env::var_os("LUM_RUNTIME_BIN") {
            let bin = PathBuf::from(path);
            ensure!(
                bin.is_absolute(),
                "LUM_RUNTIME_BIN must be an absolute directory"
            );
            return Ok(Self::External(bin));
        }
        // The packaged unit supplies a private executable store. Foreground runs
        // keep their existing per-user layout unless an explicit root is set.
        let root = env::var_os("LUM_RUNTIME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| state.join("runtimes"));
        ensure!(
            root.is_absolute(),
            "LUM_RUNTIME_ROOT must be an absolute directory"
        );
        Ok(Self::Managed(root.join(VERSION)))
    }
    pub fn bin(&self) -> PathBuf {
        match self {
            Self::Managed(dir) => dir.join("bin"),
            Self::External(bin) => bin.clone(),
        }
    }
    pub fn description(&self) -> String {
        match self {
            Self::External(bin) => format!("External runtime: {}", bin.display()),
            Self::Managed(_) => format!("Lum-managed hrx-loom {VERSION}"),
        }
    }
    pub async fn ensure(&self) -> Result<()> {
        match self {
            Self::External(bin) => verify(bin).await.context(
                "Check your LUM_RUNTIME_BIN override, or unset it for managed installation",
            ),
            Self::Managed(dir) => install(dir).await,
        }
    }
}
async fn command(cmd: &mut Command, label: &str, seconds: u64) -> Result<()> {
    // Native tools and pip print diagnostics to the foreground terminal or journal.
    // venv may spawn ensurepip. On Ctrl-C or timeout, cancel the entire
    // installer group before releasing the instance lock, not just its parent.
    struct Group(Option<rustix::process::Pid>);
    impl Drop for Group {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
        }
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Could not execute {label}"))?;
    let mut group = Group(
        child
            .id()
            .and_then(|id| rustix::process::Pid::from_raw(id as i32)),
    );
    let status = timeout(Duration::from_secs(seconds), child.wait())
        .await
        .with_context(|| format!("{label} timed out"))??;
    group.0 = None;
    ensure!(
        status.success(),
        "{label} failed ({status}); see terminal output or journalctl -u lum -b"
    );
    Ok(())
}
async fn verify(bin: &Path) -> Result<()> {
    for tool in ["loom-serve", "loom-fetch"] {
        command(
            Command::new(bin.join(tool))
                .arg("--help")
                .stdout(Stdio::null()),
            tool,
            30,
        )
        .await
        .with_context(|| format!("Checking {}", bin.join(tool).display()))?;
    }
    Ok(())
}
async fn install(dir: &Path) -> Result<()> {
    // Never move a completed venv: entry points contain absolute paths.
    let root = dir.parent().context("Missing SDK root")?;
    tokio::fs::create_dir_all(root)
        .await
        .with_context(|| format!("Create SDK root {}", root.display()))?;
    let slot = if dir.exists() { dir } else { root };
    let flags = rustix::fs::statvfs(slot)
        .with_context(|| format!("Inspect SDK mount for {}", slot.display()))?
        .f_flag;
    ensure!(
        !flags.contains(rustix::fs::StatVfsMountFlags::NOEXEC),
        "SDK location {} is mounted noexec in Lum's namespace. Use a dedicated executable LUM_RUNTIME_ROOT, not the model data mount.",
        slot.display()
    );
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".provision.lock"))?;
    lock.try_lock()
        .context("Another instance is provisioning this SDK store")?;
    let marker = dir.join("installed.lock");
    if fs::read_to_string(&marker).ok().as_deref() == Some(LOCK)
        && verify(&dir.join("bin")).await.is_ok()
    {
        return Ok(());
    }
    eprintln!("Installing managed hrx-loom {VERSION} at {}", dir.display());
    if dir.exists() {
        tokio::fs::remove_dir_all(dir)
            .await
            .context("Remove incomplete managed installation")?;
    }
    tokio::fs::create_dir_all(dir).await?;
    command(
        Command::new("/usr/bin/python3")
            .args(["-I", "-m", "venv"])
            .arg(dir),
        "creating the managed Python environment (requires Python 3.12+ with venv and ensurepip)",
        120,
    )
    .await?;
    let pending = dir.join("installing.lock");
    tokio::fs::write(&pending, LOCK).await?;
    command(
        Command::new(dir.join("bin/python"))
            .args([
                "-I",
                "-m",
                "pip",
                "--isolated",
                "--disable-pip-version-check",
                "--no-input",
                "install",
                "--index-url",
                "https://pypi.org/simple",
                "--only-binary=:all:",
                "--require-hashes",
                "-r",
            ])
            .arg(&pending),
        "installing the verified Loom wheels",
        600,
    )
    .await?;
    verify(&dir.join("bin")).await?;
    tokio::fs::rename(&pending, &marker).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refuses_noexec_before_modifying_an_existing_slot() -> Result<()> {
        // /dev/shm is normally noexec on Linux. The package CI separately
        // forces noexec on StateDirectory and exercises a real service boot.
        let shm = Path::new("/dev/shm");
        if !rustix::fs::statvfs(shm)
            .is_ok_and(|s| s.f_flag.contains(rustix::fs::StatVfsMountFlags::NOEXEC))
        {
            return Ok(());
        }
        let root = tempfile::tempdir_in(shm)?;
        let slot = root.path().join(VERSION);
        fs::create_dir(&slot)?;
        let sentinel = slot.join("existing");
        fs::write(&sentinel, "preserve")?;
        let err = install(&slot).await.unwrap_err();
        assert!(err.to_string().contains("mounted noexec"), "{err:#}");
        assert_eq!(fs::read_to_string(sentinel)?, "preserve");
        Ok(())
    }
}
