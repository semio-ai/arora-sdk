//! arora-restful: runs an HTTP-controlled robot as an Arora device.
//!
//! Usage: `arora-restful [--module <dir>]... <config.json> [overrides.json]`
//!
//! There are no built-in robots: the first argument (or the `CONFIG` env var)
//! is a path to a `RESTfulRobotConfig` JSON file describing the robot's HTTP
//! API (see `configs/hackerbot.json`). An optional second JSON file overrides
//! the selected config. Device identity, registration and token handling are
//! env-driven inside arora's Semio Studio runner.
//!
//! The device carries the modules its device directory holds under `modules/`,
//! then each `--module` directory, as the `arora` runner does
//! (`AroraBuilder::with_module_dirs`): a module directory is a module's
//! `header.json` beside its artifact, and optionally its records in
//! `records/`, which describe every function it exports.

use std::path::PathBuf;

use anyhow::{bail, Context};
use arora::Arora;
use arora_hal_restful::{RESTfulRobotConfig, RestfulHal};

const USAGE: &str = "usage: arora-restful [--module <dir>]... <config.json> [overrides.json]";

fn load_config(path: &str) -> RESTfulRobotConfig {
    let file = std::fs::File::open(path)
        .unwrap_or_else(|e| panic!("failed to open robot config {path}: {e}"));
    serde_json::from_reader(file)
        .unwrap_or_else(|e| panic!("failed to parse robot config {path}: {e}"))
}

struct Cli {
    config: String,
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
    let config = positional
        .next()
        .or_else(|| std::env::var("CONFIG").ok())
        .context(USAGE)?;
    Ok(Cli {
        config,
        overrides: positional.next(),
        modules,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    let mut config = load_config(&cli.config);
    if let Some(overrides_path) = &cli.overrides {
        config.apply_overrides(load_config(overrides_path));
    }

    // The front end first: building it installs the log sink, so what the
    // module loading below says is captured like the rest of the run.
    let builder = Arora::builder()
        .with_frontend(arora::standard_frontend())
        .with_module_dirs(&arora::device_dir::from_env()?, &cli.modules)
        .await?;
    let hal = RestfulHal::new(config)
        .map_err(|e| anyhow::anyhow!("failed to start the RESTful HAL: {e}"))?;
    builder.with_hal(Box::new(hal)).run().await
}
