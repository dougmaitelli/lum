mod config;
#[cfg(feature = "dev-ui")]
mod dev;
mod inference;
mod keys;
mod library;
mod native;
mod oidc;
mod runtime;
mod service;
mod telemetry;
mod ui;

use std::{
    env,
    process::{Command, ExitCode, Stdio},
};

const HELP: &str = "Usage:\n  lum run       Start Lum in the foreground with its own managed runtime\n  lum serve     Start the systemd-managed instance\n  lum config [--user] [KEY [VALUE]]    Read or change listener settings\n  lum doctor    Check managed-runtime prerequisites or an explicit external override";

fn doctor() -> ExitCode {
    let Some(bin) = env::var_os("LUM_RUNTIME_BIN").map(std::path::PathBuf::from) else {
        let status = Command::new("/usr/bin/python3")
            .args([
                "-I",
                "-c",
                "import ensurepip, sys, venv; sys.exit(sys.version_info < (3, 12))",
            ])
            .status();
        if status.is_ok_and(|s| s.success()) {
            println!(
                "Managed runtime: Python prerequisites available. Lum installs hrx-loom {} on first use; no Loom tools on PATH are needed.",
                runtime::VERSION
            );
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "Managed installation requires /usr/bin/python3 (3.12+) with venv and ensurepip. Install the Lum RPM or DEB for OS dependencies."
        );
        return ExitCode::FAILURE;
    };
    if !bin.is_absolute() {
        eprintln!("LUM_RUNTIME_BIN must be absolute");
        return ExitCode::FAILURE;
    }
    let mut healthy = true;
    for tool in ["loom-serve", "loom-fetch"] {
        match Command::new(bin.join(tool))
            .arg("--help")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
        {
            Ok(status) if status.success() => println!("OK: {tool}"),
            result => {
                let why = match result {
                    Ok(status) => status.to_string(),
                    Err(e) => e.to_string(),
                };
                eprintln!("Unavailable: {tool}: {why}");
                healthy = false;
            }
        }
    }
    if healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        #[cfg(feature = "dev-ui")]
        [arg] if arg == "dev" || arg == "dev-vite" => match dev::run(arg == "dev-vite") {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("UI preview failed: {e:#}");
                ExitCode::FAILURE
            }
        },
        [cmd, rest @ ..] if cmd == "config" => match config::cli(rest) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Config: {e:#}");
                ExitCode::FAILURE
            }
        },
        [arg] if arg == "doctor" => doctor(),
        [arg] if arg == "serve" || arg == "run" => match service::run(arg == "run") {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Lum service failed: {e:#}");
                ExitCode::FAILURE
            }
        },
        [] => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        [arg] if arg == "--help" || arg == "-h" => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{HELP}");
            ExitCode::from(2)
        }
    }
}
