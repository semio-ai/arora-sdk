#[test]
fn comparison_protocol() {
    let status = std::process::Command::new("python3")
        .arg("-B")
        .arg("scripts/test_analyze.py")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("python3 is required for benchmark comparison checks");
    assert!(status.success());
}
