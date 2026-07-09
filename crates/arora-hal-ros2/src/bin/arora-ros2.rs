//! arora-ros2: runs a ROS 2 robot as an Arora device.
//!
//! Usage: `arora-ros2 <nao|pepper|quori|ur3|ur5|g1|config.json> [overrides.json]`
//!
//! The robot is picked at runtime: the first argument (or the `ROBOT` env var)
//! is a well-known robot name or a path to a `ROS2RobotConfig` JSON file. An
//! optional second JSON file overrides the selected config. `ROS_DOMAIN_ID`
//! overrides the config's domain id. Device identity, registration and token
//! handling are env-driven inside arora's Semio Studio runner.

use arora_hal_ros2::{configs, ROS2RobotConfig, Ros2Hal};

fn load_config(selector: &str) -> ROS2RobotConfig {
    match selector {
        "nao" => configs::nao::create_config(),
        "pepper" => configs::pepper::create_config(),
        "quori" => configs::quori::create_config(),
        "ur3" => configs::ur3::create_config(),
        "ur5" => configs::ur5::create_config(),
        "g1" => configs::unitree_g1::create_config(),
        path => {
            let file = std::fs::File::open(path)
                .unwrap_or_else(|e| panic!("failed to open robot config {path}: {e}"));
            serde_json::from_reader(file)
                .unwrap_or_else(|e| panic!("failed to parse robot config {path}: {e}"))
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let selector = args
        .next()
        .or_else(|| std::env::var("ROBOT").ok())
        .expect("usage: arora-ros2 <nao|pepper|quori|ur3|ur5|g1|config.json> [overrides.json]");
    let mut config = load_config(&selector);
    if let Some(overrides_path) = args.next() {
        config.apply_overrides(load_config(&overrides_path));
    }
    if let Ok(domain_id) = std::env::var("ROS_DOMAIN_ID") {
        config.domain_id = Some(domain_id.parse().expect("ROS_DOMAIN_ID must be a number"));
    }

    // One runtime carries everything: the HAL's ROS tasks spawn on it at
    // construction, and arora drives the device on it.
    let hal = Ros2Hal::new(config)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start the ROS 2 HAL: {e}"))?;
    arora::run_with_hal(Box::new(hal)).await
}
