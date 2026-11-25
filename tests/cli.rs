use std::process::Command;

#[test]
fn help_works_without_runtime() {
    let out = Command::new(env!("CARGO_BIN_EXE_lum"))
        .arg("--help")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("lum doctor"));
}

#[test]
fn service_requires_a_runtime_directory() {
    let out = Command::new(env!("CARGO_BIN_EXE_lum"))
        .arg("serve")
        .env_remove("RUNTIME_DIRECTORY")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("RUNTIME_DIRECTORY"));
}

#[test]
fn missing_external_tools_are_reported() {
    let out = Command::new(env!("CARGO_BIN_EXE_lum"))
        .arg("doctor")
        .env("LUM_RUNTIME_BIN", "/nonexistent")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let diagnostics = String::from_utf8_lossy(&out.stderr);
    for tool in ["loom-serve", "loom-fetch"] {
        assert!(diagnostics.contains(tool));
    }
}
