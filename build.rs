use std::{env, path::Path, process::Command};

fn main() {
    for path in [
        "src",
        "index.html",
        "public",
        "package.json",
        "package-lock.json",
        "vite.config.js",
        "tsconfig.json",
    ] {
        println!("cargo:rerun-if-changed=web/{path}");
    }
    assert!(
        Path::new("web/node_modules/vite/bin/vite.js").is_file(),
        "Install frontend build dependencies first: npm ci --prefix web"
    );
    let out = Path::new(&env::var_os("OUT_DIR").unwrap()).join("ui");
    let status = Command::new("node")
        .current_dir("web")
        .args(["node_modules/vite/bin/vite.js", "build", "--outDir"])
        .arg(out)
        .status()
        .expect("Node.js is required to build the embedded web UI");
    assert!(status.success(), "Vite build failed");
}
