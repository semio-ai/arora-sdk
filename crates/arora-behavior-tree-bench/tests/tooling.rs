#[test]
fn comparison_protocol() {
    for script in ["scripts/test_analyze.py", "scripts/test_run.py"] {
        let status = std::process::Command::new("python3")
            .arg("-B")
            .arg(script)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status()
            .expect("python3 is required for benchmark comparison checks");
        assert!(status.success(), "{script}");
    }
}
