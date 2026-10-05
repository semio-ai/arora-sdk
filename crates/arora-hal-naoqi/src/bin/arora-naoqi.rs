//! arora-naoqi: runs a NAOqi robot (NAO, Pepper) as an Arora device.
//!
//! Usage: `arora-naoqi [--tree <tree.groot.xml> | --no-tree] <tcp://host:9559 | config.json> [overrides.json]`
//!
//! The robot is picked at runtime: the first argument (or the `ROBOT` env var) is the
//! address of its NAOqi or a path to a `NaoqiRobotConfig` JSON file (see
//! `configs/nao.json`). An optional second JSON file overrides the selected config. Device
//! identity, registration and token handling are env-driven inside arora's Semio Studio
//! runner.
//!
//! The device runs a behavior tree: by default `trees/ready.groot.xml`, which says "Arora is
//! ready" with a hand gesture at start; `--tree` runs another one, `--no-tree` none. The
//! tree's leaves are the robot's voice (`Say`), its gestures (`HandsReady`) and the
//! blackboard's `Equals` and `Assign`. On the terminal UI, `r` sets `command.behavior` to
//! `ready`, which plays the default behavior again; the key is open to remote writers too.

use std::time::Duration;

use anyhow::{bail, Context};
use arora::tui::{commands_frontend, TuiCommand};
use arora::{Arora, HostModule};
use arora_hal::Hal;
use arora_hal_naoqi::{
    gestures::naoqi_gestures, ready_tree_initial_state, say, say::naoqi_say, NaoqiHal,
    NaoqiRobotConfig, COMMAND_BEHAVIOR, READY_TREE,
};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, KeyMeta, StateChange};
use arora_types::value::Value;
use futures::StreamExt;

const USAGE: &str = "usage: arora-naoqi [--tree <tree.groot.xml> | --no-tree] \
                     <tcp://host:9559 | config.json> [overrides.json]";

/// The step period: 50 Hz, a pace NAOqi's `setAngles` round trips keep up with.
const STEP_PERIOD: Duration = Duration::from_millis(20);

/// The terminal UI key that plays the default behavior again.
const READY_KEY: char = 'r';

fn load_config(selector: &str) -> anyhow::Result<NaoqiRobotConfig> {
    if selector.starts_with("tcp://") || selector.starts_with("tcps://") {
        return Ok(NaoqiRobotConfig::with_url(selector));
    }
    let file = std::fs::File::open(selector)
        .with_context(|| format!("failed to open robot config {selector}"))?;
    serde_json::from_reader(file)
        .with_context(|| format!("failed to parse robot config {selector}"))
}

struct Cli {
    tree: Option<String>,
    selector: String,
    overrides: Option<String>,
}

fn parse_cli() -> anyhow::Result<Cli> {
    let mut tree = Some(READY_TREE.to_string());
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tree" => {
                let path = args.next().context(USAGE)?;
                tree = Some(
                    std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read the tree {path}"))?,
                );
            }
            "--no-tree" => tree = None,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
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
        tree,
        selector,
        overrides: positional.next(),
    })
}

fn ready() -> StateChange {
    StateChange::set(COMMAND_BEHAVIOR, Value::String("ready".to_string()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    let mut config = load_config(&cli.selector)?;
    if let Some(overrides) = &cli.overrides {
        config.apply_overrides(load_config(overrides)?);
    }

    // The front end first: it installs the log sink, so the connection's logs are shown.
    let (frontend, mut commands) = commands_frontend(vec![TuiCommand {
        key: READY_KEY,
        label: "say \"Arora is ready\"".to_string(),
        prompt: None,
    }]);

    // One runtime carries everything: the HAL's tasks spawn on it at construction, the
    // voice speaks on it, and arora drives the device on it.
    let hal = NaoqiHal::new(config)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start the NAOqi HAL: {e}"))?;
    say::install(hal.voice());

    // The default behavior starts at once; the request key is open to remote writers, so
    // Studio or a bridge client can play it again as the terminal UI does.
    let store = SimpleDataStore::new();
    if cli.tree.is_some() {
        let state = hal
            .read_all()
            .await
            .map_err(|e| anyhow::anyhow!("failed to read the robot's state: {e}"))?;
        store.write(ready_tree_initial_state(&state))?;
    }
    store.set_meta([(Key::from(COMMAND_BEHAVIOR), KeyMeta::new().editable())].into())?;
    let requests = store.clone();
    tokio::spawn(async move {
        while let Some(command) = commands.next().await {
            if command.key == READY_KEY {
                if let Err(error) = requests.write(ready()) {
                    log::warn!("could not request the ready behavior: {error}");
                }
            }
        }
    });

    let mut builder = Arora::builder()
        .with_frontend(frontend)
        .with_data_store(Box::new(store))
        .with_hal(Box::new(hal))
        .with_host_module(HostModule::of::<naoqi_say::Module>())
        .with_host_module(HostModule::of::<naoqi_gestures::Module>())
        .with_host_module(HostModule::of::<blackboard::blackboard::Module>())
        .with_step_period(STEP_PERIOD);
    if let Some(tree) = cli.tree {
        builder = builder.with_groot(tree);
    }
    builder.run().await
}
