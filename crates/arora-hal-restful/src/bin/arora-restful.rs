//! arora-restful: runs an HTTP-controlled robot as an Arora device.
//!
//! Usage: `arora-restful <config.json> [overrides.json]`
//!
//! There are no built-in robots: the first argument (or the `CONFIG` env var)
//! is a path to a `RESTfulRobotConfig` JSON file describing the robot's HTTP
//! API (see `configs/hackerbot.json`). An optional second JSON file overrides
//! the selected config. Device identity, registration and token handling are
//! env-driven inside arora's Semio Studio runner.

use std::sync::Arc;

use arora_hal_restful::{RESTfulRobotConfig, RestfulHal};

fn load_config(path: &str) -> RESTfulRobotConfig {
    let file = std::fs::File::open(path)
        .unwrap_or_else(|e| panic!("failed to open robot config {path}: {e}"));
    serde_json::from_reader(file)
        .unwrap_or_else(|e| panic!("failed to parse robot config {path}: {e}"))
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .or_else(|| std::env::var("CONFIG").ok())
        .expect("usage: arora-restful <config.json> [overrides.json]");
    let mut config = load_config(&path);
    if let Some(overrides_path) = args.next() {
        config.apply_overrides(load_config(&overrides_path));
    }

    let hal = RestfulHal::new(config)
        .map_err(|e| anyhow::anyhow!("failed to start the RESTful HAL: {e}"))?;
    arora::run_with_hal(Arc::new(hal))
}
