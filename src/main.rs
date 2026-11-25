mod library;
mod native;
mod service;
mod ui;
mod view;

use std::{
    env,
    process::{Command, ExitCode, Stdio},
};

const HELP: &str = "Usage:\n  lum doctor    Check Loom tools on PATH\n  lum serve     Run the local web library and private engine";

fn doctor() -> ExitCode {
    let mut healthy = true;
    for tool in ["loom-serve", "loom-fetch"] {
        match Command::new(tool)
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
        [arg] if arg == "doctor" => doctor(),
        [arg] if arg == "serve" => match service::run() {
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
