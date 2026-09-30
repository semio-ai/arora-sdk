//! arora-naoqi: runs a NAOqi robot (NAO, Pepper) as an Arora device.
//!
//! Usage: `arora-naoqi <tcp://host:9559 | config.json> [overrides.json]`
//!
//! The robot is picked at runtime: the first argument (or the `ROBOT` env var) is the
//! address of its NAOqi or a path to a `NaoqiRobotConfig` JSON file (see
//! `configs/nao.json`). An optional second JSON file overrides the selected config. Device
//! identity, registration and token handling are env-driven inside arora's Semio Studio
//! runner.

use arora_hal_naoqi::{NaoqiHal, NaoqiRobotConfig};

fn load_config(selector: &str) -> NaoqiRobotConfig {
    if selector.starts_with("tcp://") {
        return NaoqiRobotConfig::with_url(selector);
    }
    let file = std::fs::File::open(selector)
        .unwrap_or_else(|e| panic!("failed to open robot config {selector}: {e}"));
    serde_json::from_reader(file)
        .unwrap_or_else(|e| panic!("failed to parse robot config {selector}: {e}"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let selector = args
        .next()
        .or_else(|| std::env::var("ROBOT").ok())
        .expect("usage: arora-naoqi <tcp://host:9559 | config.json> [overrides.json]");
    let mut config = load_config(&selector);
    if let Some(overrides_path) = args.next() {
        config.apply_overrides(load_config(&overrides_path));
    }

    // One runtime carries everything: the HAL's tasks spawn on it at construction, and
    // arora drives the device on it.
    let hal = NaoqiHal::new(config)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start the NAOqi HAL: {e}"))?;
    arora::run_with_hal(Box::new(hal)).await
}
