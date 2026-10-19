//! arora-ros2: runs a ROS 2 robot as an Arora device.
//!
//! Usage: `arora-ros2 [--module <dir>]... <nao|pepper|quori|ur3|ur5|g1|config.json> [overrides.json]`
//!
//! The robot is picked at runtime: the first argument (or the `ROBOT` env var)
//! is a well-known robot name or a path to a `ROS2RobotConfig` JSON file. An
//! optional second JSON file overrides the selected config. `ROS_DOMAIN_ID`
//! overrides the config's domain id. Device identity, registration and token
//! handling are env-driven inside arora's Semio Studio runner.
//!
//! The device carries the modules its device directory holds under `modules/`,
//! then each `--module` directory, as the `arora` runner does
//! (`AroraBuilder::with_module_dirs`): a module directory is a module's
//! `header.json` beside its artifact, and optionally its records in
//! `records/`, which describe every function it exports.

use std::path::PathBuf;

use anyhow::{bail, Context};
use arora::Arora;
use arora_hal_ros2::{configs, ROS2RobotConfig, Ros2Hal};

const USAGE: &str = "usage: arora-ros2 [--module <dir>]... \
                     <nao|pepper|quori|ur3|ur5|g1|config.json> [overrides.json]";

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

struct Cli {
    selector: String,
    overrides: Option<String>,
    modules: Vec<PathBuf>,
}

fn parse_cli() -> anyhow::Result<Cli> {
    let mut modules = Vec::new();
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--module" => modules.push(PathBuf::from(args.next().context(USAGE)?)),
            flag if flag.starts_with("--") => bail!("unknown option {flag}\n{USAGE}"),
            _ => positional.push(arg),
        }
    }
    let mut positional = positional.into_iter();
    let selector = positional
        .next()
        .or_else(|| std::env::var("ROBOT").ok())
        .context(USAGE)?;
    Ok(Cli {
        selector,
        overrides: positional.next(),
        modules,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    let mut config = load_config(&cli.selector);
    if let Some(overrides_path) = &cli.overrides {
        config.apply_overrides(load_config(overrides_path));
    }
    if let Ok(domain_id) = std::env::var("ROS_DOMAIN_ID") {
        config.domain_id = Some(domain_id.parse().expect("ROS_DOMAIN_ID must be a number"));
    }

    // The front end first: building it installs the log sink, so what the
    // module loading and the HAL say is captured like the rest of the run. The
    // modules the device carries are read before the robot is reached: a
    // module directory that does not read fails the start at once.
    let builder = Arora::builder()
        .with_frontend(arora::standard_frontend())
        .with_module_dirs(&arora::device_dir::from_env()?, &cli.modules)
        .await?;
    // One runtime carries everything: the HAL's ROS tasks spawn on it at
    // construction, and arora drives the device on it.
    let hal = Ros2Hal::new(config)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start the ROS 2 HAL: {e}"))?;
    builder.with_hal(Box::new(hal)).run().await
}
