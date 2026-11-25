//! Persistent listener settings shared by the CLI, foreground runner and service.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions, Permissions},
    io::{ErrorKind, Read, Write},
    net::{IpAddr, Ipv4Addr},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const SYSTEM: &str = "/etc/lum/config.json";
const USAGE: &str = "Usage: lum config [--user] [api.host|api.port|api.name|inference.host|inference.port|models.max_loaded [VALUE]]\nUse --user for lum run, otherwise settings target lum.service. Use '-' to clear api.name.";

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Saved {
    #[serde(rename = "api.host", skip_serializing_if = "Option::is_none")]
    host: Option<IpAddr>,
    #[serde(rename = "api.port", skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(rename = "api.name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(rename = "inference.host", skip_serializing_if = "Option::is_none")]
    inference_host: Option<IpAddr>,
    #[serde(rename = "inference.port", skip_serializing_if = "Option::is_none")]
    inference_port: Option<u16>,
    #[serde(rename = "models.max_loaded", skip_serializing_if = "Option::is_none")]
    max_loaded: Option<std::num::NonZeroUsize>,
}
#[derive(Clone)]
pub struct Listener {
    pub max_loaded: usize,
    pub inference: std::net::SocketAddr,
    pub host: IpAddr,
    pub port: u16,
    pub name: Option<String>,
}
impl Listener {
    pub fn load(user: bool) -> Result<Self> {
        Saved::load(&path(user)?)?.listener()
    }
}

fn path(user: bool) -> Result<PathBuf> {
    let path = if let Some(p) = env::var_os("LUM_CONFIG_FILE") {
        PathBuf::from(p)
    } else if user {
        let base = env::var_os("XDG_CONFIG_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
            .context("HOME or XDG_CONFIG_HOME required")?;
        base.join("lum/config.json")
    } else {
        PathBuf::from(SYSTEM)
    };
    ensure!(path.is_absolute(), "Config path must be absolute");
    Ok(path)
}
fn dns_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.as_bytes()[0].is_ascii_alphanumeric()
                && l.as_bytes()[l.len() - 1].is_ascii_alphanumeric()
                && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
}
impl Saved {
    fn load(path: &Path) -> Result<Self> {
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(flags.bits() as i32)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("Read {}", path.display())),
        };
        ensure!(file.metadata()?.is_file(), "Config must be a regular file");
        let mut bytes = Vec::new();
        file.take(65537).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65536, "Config is too large");
        serde_json::from_slice(&bytes)
            .with_context(|| format!("Invalid config in {}", path.display()))
    }
    fn listener(&self) -> Result<Listener> {
        let host = self.host.unwrap_or(Ipv4Addr::LOCALHOST.into());
        let port = match self.port {
            Some(0) => bail!("api.port must be 1–65535"),
            Some(p) => p,
            None => env::var("LUM_ADMIN_PORT")
                .unwrap_or_else(|_| "9390".into())
                .parse()
                .context("Invalid LUM_ADMIN_PORT")?,
        };
        if let Some(n) = &self.name {
            ensure!(
                dns_name(n),
                "api.name must be a hostname without a scheme or port"
            );
        }
        let inference_port = match self.inference_port {
            Some(0) => bail!("inference.port must be 1–65535"),
            Some(p) => p,
            None => env::var("LUM_INFERENCE_PORT")
                .unwrap_or_else(|_| "9391".into())
                .parse()
                .context("Invalid LUM_INFERENCE_PORT")?,
        };
        Ok(Listener {
            max_loaded: self.max_loaded.map_or(1, |n| n.get()),
            inference: std::net::SocketAddr::new(
                self.inference_host.unwrap_or(Ipv4Addr::LOCALHOST.into()),
                inference_port,
            ),
            host,
            port,
            name: self.name.clone(),
        })
    }
}
pub fn cli(args: &[OsString]) -> Result<()> {
    let user = args.first().is_some_and(|a| a == "--user");
    let args = args[usize::from(user)..]
        .iter()
        .map(|s| s.to_str().context("Settings require UTF-8"))
        .collect::<Result<Vec<_>>>()?;
    if args == ["--help"] {
        println!("{USAGE}");
        return Ok(());
    }
    let path = path(user)?;
    if args.is_empty()
        || matches!(
            args.as_slice(),
            ["api.host"
                | "api.port"
                | "api.name"
                | "inference.host"
                | "inference.port"
                | "models.max_loaded"]
        )
    {
        let l = Saved::load(&path)?.listener()?;
        let name = l.name.as_deref().unwrap_or("-");
        match args.first().copied() {
            Some("models.max_loaded") => println!("{}", l.max_loaded),
            Some("api.host") => println!("{}", l.host),
            Some("api.port") => println!("{}", l.port),
            Some("api.name") => println!("{name}"),
            Some("inference.host") => println!("{}", l.inference.ip()),
            Some("inference.port") => println!("{}", l.inference.port()),
            _ => println!(
                "api:\n  host: {}\n  port: {}\n  name: {name}\ninference:\n  host: {}\n  port: {}\nmodels:\n  max_loaded: {}",
                l.host,
                l.port,
                l.inference.ip(),
                l.inference.port(),
                l.max_loaded
            ),
        };
        return Ok(());
    }
    ensure!(
        matches!(
            args.as_slice(),
            [
                "api.host"
                    | "api.port"
                    | "api.name"
                    | "inference.host"
                    | "inference.port"
                    | "models.max_loaded",
                _
            ]
        ),
        "{USAGE}"
    );
    let parent = path.parent().context("Invalid config path")?;
    fs::create_dir_all(parent)?;
    // Serialize atomic updates without a separate writable lock file.
    let lock = File::open(parent)
        .with_context(|| format!("Open configuration directory {}", parent.display()))?;
    lock.try_lock()
        .context("Configuration is busy; retry the command")?;
    let mut saved = Saved::load(&path)?;
    match args.as_slice() {
        ["api.host", v] => {
            saved.host = Some(
                v.parse()
                    .context("api.host must be an IPv4 or IPv6 address")?,
            )
        }
        ["api.port", v] => saved.port = Some(v.parse().context("api.port must be 1–65535")?),
        ["inference.host", v] => {
            saved.inference_host = Some(v.parse().context("inference.host must be an IP address")?)
        }
        ["inference.port", v] => {
            saved.inference_port = Some(v.parse().context("inference.port must be 1–65535")?)
        }
        ["models.max_loaded", v] => {
            saved.max_loaded = Some(
                v.parse()
                    .context("models.max_loaded must be a positive integer")?,
            )
        }
        ["api.name", v] => saved.name = (*v != "-").then(|| v.to_ascii_lowercase()),
        _ => unreachable!(),
    }
    saved.listener()?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent).with_context(|| {
        if path == Path::new(SYSTEM) {
            format!(
                "Cannot write {} (use sudo for system settings)",
                path.display()
            )
        } else {
            format!("Cannot write {}", path.display())
        }
    })?;
    serde_json::to_writer_pretty(&mut tmp, &saved)?;
    writeln!(tmp)?;
    tmp.as_file()
        .set_permissions(Permissions::from_mode(if user { 0o600 } else { 0o644 }))?;
    tmp.as_file().sync_all()?;
    tmp.persist(&path)?;
    lock.sync_all()?;
    if saved.host.is_some_and(|h| !h.is_loopback()) {
        eprintln!(
            "LAN access has no login. Anyone who can reach this listener can manage your models."
        );
    }
    println!(
        "Saved {}. {}",
        path.display(),
        if user {
            "Restart lum run to apply."
        } else {
            "Run sudo systemctl restart lum.service to apply."
        }
    );
    Ok(())
}
