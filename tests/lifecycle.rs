#[path = "support/engine.rs"]
mod engine;
#[path = "support/smoke.rs"]
mod smoke;

use std::{fs, path::Path};

fn fixture() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/loom-text"
    ))
}

async fn check(package: &Path, binary: &str) -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let result = smoke::run(package, Path::new(binary), tmp.path()).await;
    assert_eq!(
        fs::read_dir(tmp.path())?.count(),
        0,
        "private runtime directory leaked"
    );
    result
}

#[tokio::test]
async fn child_exit_is_reported_even_before_readiness() {
    let err = check(fixture(), "/bin/false").await.unwrap_err();
    assert!(
        err.to_string().contains("loom-serve exited unexpectedly"),
        "{err:#}"
    );
}

#[tokio::test]
#[ignore = "requires native hrx-loom 0.3.0 loom-serve on PATH"]
async fn two_cpu_models_survive_peer_retirement() -> anyhow::Result<()> {
    check(fixture(), "loom-serve").await
}

#[tokio::test]
#[ignore = "requires native hrx-loom 0.3.0 loom-serve on PATH"]
async fn accepted_but_unpreparable_card_fails_the_check() {
    let broken = tempfile::tempdir().unwrap();
    fs::copy(
        fixture().join("model.json"),
        broken.path().join("model.json"),
    )
    .unwrap();
    let err = check(broken.path(), "loom-serve").await.unwrap_err();
    assert!(err.to_string().contains("alpha: failed"), "{err:#}");
}
