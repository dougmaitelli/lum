use reqwest::{Client, StatusCode, header};
use std::{
    fs,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Output, Stdio},
    time::Duration,
};
use tokio::time::{sleep, timeout};

fn config(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lum"))
        .env_remove("LUM_CONFIG_FILE")
        .env_remove("LUM_ADMIN_PORT")
        .env_remove("LUM_INFERENCE_PORT")
        .env("XDG_CONFIG_HOME", dir)
        .args(["config", "--user"])
        .args(args)
        .output()
        .unwrap()
}
fn ok(dir: &Path, args: &[&str]) -> String {
    let out = config(dir, args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn print_hierarchy_and_validate_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    assert_eq!(ok(dir, &["api.host"]).trim(), "127.0.0.1");
    assert_eq!(ok(dir, &["api.port"]).trim(), "9391");
    assert_eq!(ok(dir, &["admin.port"]).trim(), "9390");
    assert_eq!(ok(dir, &["models.max_loaded"]).trim(), "1");
    ok(dir, &["models.max_loaded", "3"]);
    assert_eq!(ok(dir, &["models.max_loaded"]).trim(), "3");
    ok(dir, &["admin.port", "9494"]);
    let file = dir.join("lum/config.json");
    let original = fs::read(&file).unwrap();
    for args in [
        ["models.max_loaded", "0"],
        ["models.max_loaded", "-1"],
        ["api.port", "65536"],
        ["api.port", "0"],
        ["api.host", "*"],
        ["admin.port", "0"],
        ["admin.host", "*"],
        ["admin.name", "https://ai"],
        ["oidc.enabled", "true"],
        ["oidc.origin", "http://lum.example"],
        ["oidc.issuer", "https://id.example/#bad"],
        ["oidc.client_secret_file", "relative"],
        ["unknown", "value"],
    ] {
        assert!(!config(dir, &args).status.success());
        assert_eq!(fs::read(&file).unwrap(), original);
    }
    assert_eq!(ok(dir, &["admin.port"]).trim(), "9494");
    let out = ok(dir, &["admin.host", "0.0.0.0"]);
    assert!(!out.contains("password"));
    assert!(!fs::read_to_string(file).unwrap().contains("credential"));
    assert_eq!(
        ok(dir, &[]),
        "admin:\n  host: 0.0.0.0\n  port: 9494\n  name: -\napi:\n  host: 127.0.0.1\n  port: 9391\nmodels:\n  max_loaded: 3\noidc:\n  enabled: false\n  issuer: -\n  origin: -\n  client_id: -\n  allowed_subjects: -\n  client_secret_file: -\n"
    );
    assert!(!config(dir, &["--reset-password"]).status.success());
}
#[test]
fn api_binding_is_independent_of_admin() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    ok(dir, &["api.host", "0.0.0.0"]);
    assert_eq!(ok(dir, &["api.host"]).trim(), "0.0.0.0");
    assert_eq!(ok(dir, &["admin.host"]).trim(), "127.0.0.1");
    assert!(!config(dir, &["inference.host"]).status.success());
}
#[tokio::test]
async fn oidc_gates_setup_and_admin_without_contacting_provider() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    for (key, value) in [
        ("oidc.issuer", "https://id.example"),
        ("oidc.origin", "https://lum.example"),
        ("oidc.client_id", "lum"),
        ("oidc.allowed_subjects", "owner"),
        ("oidc.enabled", "true"),
        ("admin.port", &port.to_string()),
    ] {
        ok(dir, &[key, value]);
    }
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_lum"))
            .arg("run")
            .env_remove("LUM_CONFIG_FILE")
            .env("XDG_CONFIG_HOME", dir)
            .env("XDG_DATA_HOME", dir.join("data"))
            .env("XDG_RUNTIME_DIR", dir.join("run"))
            .env("LUM_INFERENCE_PORT", "0")
            .env("LUM_RUNTIME_BIN", dir.join("no-sdk"))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let http = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let get = |path: &str| {
        http.get(format!("{base}{path}"))
            .header("Host", "lum.example")
    };
    timeout(Duration::from_secs(5), async {
        while get("/login").send().await.is_err() {
            sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap();
    assert_eq!(get("/login").send().await.unwrap().status(), 200);
    assert_eq!(
        get("/").send().await.unwrap().headers()[header::LOCATION],
        "/login"
    );
    for path in ["/admin/state", "/admin/api", "/admin/runtime", "/metrics"] {
        assert_eq!(get(path).send().await.unwrap().status(), 401);
    }
    for path in [
        "/admin/runtime/retry",
        "/admin/keys",
        "/admin/models/example/start",
    ] {
        assert_eq!(
            http.post(format!("{base}{path}"))
                .header("Host", "lum.example")
                .header("Origin", "https://lum.example")
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        http.post(format!("{base}/auth/logout"))
            .header("Host", "lum.example")
            .header("Origin", "https://other.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        http.get(format!("{base}/admin/runtime"))
            .header("X-Forwarded-Host", "lum.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
}
#[tokio::test]
async fn saved_lan_bind_needs_no_login_but_checks_host_and_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    ok(dir, &["admin.host", "0.0.0.0"]);
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    ok(dir, &["admin.port", &port.to_string()]);
    ok(dir, &["admin.name", "ai"]);
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_lum"))
            .arg("run")
            .env_remove("LUM_CONFIG_FILE")
            .env("XDG_CONFIG_HOME", dir)
            .env("XDG_DATA_HOME", dir.join("data"))
            .env("XDG_RUNTIME_DIR", dir.join("run"))
            .env("LUM_ADMIN_PORT", "0")
            .env("LUM_INFERENCE_PORT", "0")
            .env("LUM_RUNTIME_BIN", dir.join("no-sdk"))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = Client::builder().no_proxy().build().unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    let css = format!("{origin}/assets/index.css");
    let r = timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(r) = client.get(&css).send().await {
                break r;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(!r.headers().contains_key(header::WWW_AUTHENTICATE));
    for (host, status) in [("ai", 200), ("127.0.0.1", 200), ("untrusted.test", 403)] {
        assert_eq!(
            client
                .get(&css)
                .header("Host", format!("{host}:{port}"))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            status
        );
    }
    assert_eq!(
        client
            .post(format!("{origin}/admin/runtime/retry"))
            .header(header::ORIGIN, "http://untrusted.test")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
