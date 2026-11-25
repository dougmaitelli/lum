//! Persistent listener settings shared by the CLI, foreground runner and service.
use anyhow::{Context, Result, bail, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ring::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions, Permissions},
    io::{ErrorKind, Write},
    net::{IpAddr, Ipv4Addr},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use subtle::ConstantTimeEq;

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Saved {
    #[serde(rename = "api.host", skip_serializing_if = "Option::is_none")]
    host: Option<IpAddr>,
    #[serde(rename = "api.port", skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(rename = "api.name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    // Random 256-bit password; only the digest of its Basic credential is stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    credential_sha256: Option<String>,
}

#[derive(Clone)]
pub struct Listener {
    pub host: IpAddr,
    pub port: u16,
    pub name: Option<String>,
    hash: Option<Vec<u8>>,
}
impl Listener {
    pub fn load(user: bool) -> Result<Self> {
        Saved::read(&path(user)?)?.listener()
    }
    pub fn authenticated(&self, authorization: Option<&str>) -> bool {
        if self.host.is_loopback() {
            return true;
        }
        let Some((scheme, token)) = authorization.and_then(|h| h.split_once(' ')) else {
            return false;
        };
        scheme.eq_ignore_ascii_case("basic")
            && self
                .hash
                .as_ref()
                .is_some_and(|h| bool::from(digest(&SHA256, token.as_bytes()).as_ref().ct_eq(h)))
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
        PathBuf::from("/etc/lum/config.json")
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
    fn read(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(v) => serde_json::from_slice(&v)
                .with_context(|| format!("Invalid config in {}", path.display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("Read {}", path.display())),
        }
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
        let hash = self
            .credential_sha256
            .as_ref()
            .map(|h| STANDARD.decode(h))
            .transpose()
            .context("Invalid admin credential digest")?;
        ensure!(
            hash.as_ref().is_none_or(|h| h.len() == 32),
            "Invalid admin credential digest"
        );
        ensure!(
            host.is_loopback() || hash.is_some(),
            "LAN binding requires an admin password: use `lum config --reset-password` for this config"
        );
        Ok(Listener {
            host,
            port,
            name: self.name.clone(),
            hash,
        })
    }
    fn password(&mut self) -> Result<String> {
        let mut key = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| anyhow::anyhow!("System randomness unavailable"))?;
        let pass = URL_SAFE_NO_PAD.encode(key);
        let token = STANDARD.encode(format!("lum:{pass}"));
        self.credential_sha256 = Some(STANDARD.encode(digest(&SHA256, token.as_bytes()).as_ref()));
        Ok(pass)
    }
}
const USAGE: &str = "Usage: lum config [--user] [api.host|api.port|api.name [VALUE] | --reset-password]\nUse `--user` for lum run, otherwise settings target lum.service. Use '-' to clear api.name.";
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
    if args.is_empty() || matches!(args.as_slice(), ["api.host" | "api.port" | "api.name"]) {
        let l = Saved::read(&path)?.listener()?;
        let name = l.name.as_deref().unwrap_or("-");
        match args.first().copied() {
            Some("api.host") => println!("{}", l.host),
            Some("api.port") => println!("{}", l.port),
            Some("api.name") => println!("{name}"),
            _ => println!(
                "{}\napi.host = {}\napi.port = {}\napi.name = {name}",
                path.display(),
                l.host,
                l.port
            ),
        };
        return Ok(());
    }
    ensure!(
        matches!(
            args.as_slice(),
            ["api.host" | "api.port" | "api.name", _] | ["--reset-password"]
        ),
        "{USAGE}"
    );
    let parent = path.parent().context("Invalid config path")?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))
        .with_context(|| {
            format!(
                "Cannot write {} (use sudo for the system config)",
                path.display()
            )
        })?;
    lock.lock()?;
    let mut saved = Saved::read(&path)?;
    let mut password = None;
    match args.as_slice() {
        ["api.host", v] => {
            let host = v
                .parse::<IpAddr>()
                .context("api.host must be an IPv4 or IPv6 address")?;
            saved.host = Some(host);
            if !host.is_loopback() && saved.credential_sha256.is_none() {
                password = Some(saved.password()?);
            }
        }
        ["api.port", v] => saved.port = Some(v.parse().context("api.port must be 1–65535")?),
        ["api.name", v] => saved.name = (*v != "-").then(|| v.to_ascii_lowercase()),
        ["--reset-password"] => password = Some(saved.password()?),
        _ => unreachable!(),
    }
    saved.listener()?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut tmp, &saved)?;
    writeln!(tmp)?;
    // The service is unprivileged and must be able to read the system config.
    tmp.as_file()
        .set_permissions(Permissions::from_mode(if user { 0o600 } else { 0o644 }))?;
    tmp.as_file().sync_all()?;
    tmp.persist(&path)?;
    if let Some(pass) = password {
        println!("Admin username: lum\nAdmin password (shown once): {pass}");
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
