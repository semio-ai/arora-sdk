//! arora-naoqi: runs a NAOqi robot (NAO, Pepper) as an Arora device.
//!
//! Usage: `arora-naoqi [--tree <FILE> | --no-tree] [--module <DIR>]... [--studio | --no-studio]
//! <ROBOT> [OVERRIDES]` (`--help` lists the options).
//!
//! The robot is picked at runtime: the first argument (or the `ROBOT` env var) is the
//! address of its NAOqi or a path to a `NaoqiRobotConfig` JSON file (see
//! `configs/nao.json`). An optional second JSON file overrides the selected config.
//!
//! The device serves the open local bridge, and connects to Semio Studio instead when it
//! holds a saved Studio credential or the run passes `--studio` (`ARORA_STUDIO=1`), which
//! registers a device holding none; `--no-studio` keeps it on the local bridge. Its identity,
//! registration and credential are arora's (`arora::studio`), configured from the
//! environment.
//!
//! The device runs a behavior tree: by default `trees/ready.groot.xml`, which says "Arora is
//! ready" with a hand gesture at start; `--tree` runs another one, `--no-tree` none. The
//! tree's leaves are the robot's voice (`Say`), its gestures (`HandsReady`) and the
//! native `IsStringSet` and `UnsetString`. On the terminal UI, `r` requests it again through
//! `ready`, which plays the default behavior again; the key is open to remote writers too.
//!
//! The device carries the modules its device directory holds under `modules/`, then each
//! `--module` directory, as the `arora` runner does (`AroraBuilder::with_module_dirs`): a
//! module directory is a module's `header.json` beside its artifact, and optionally its
//! records in `records/`, which describe every function it exports. A tree's leaves call
//! their functions as they call `Say` and `HandsReady`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use arora::tui::{commands_frontend, TuiCommand};
use arora::{Arora, HostModule, RunnerArgs};
use arora_hal::Hal;
use arora_hal_naoqi::{
    gestures::naoqi_gestures, ready_tree_initial_state, say, say::naoqi_say, NaoqiHal,
    NaoqiRobotConfig, COMMAND_READY, READY_REQUEST, READY_TREE,
};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, KeyMeta, StateChange};
use arora_types::value::Value;
use clap::Parser;
use futures::StreamExt;

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

/// The runner's command line.
#[derive(Debug, Parser)]
#[command(
    name = "arora-naoqi",
    about = "Runs a NAOqi robot (NAO, Pepper) as an Arora device"
)]
struct Cli {
    /// A Groot behavior tree to run instead of the default one.
    #[arg(long, value_name = "FILE", conflicts_with = "no_tree")]
    tree: Option<PathBuf>,
    /// Run no behavior tree.
    #[arg(long)]
    no_tree: bool,
    /// The robot: its NAOqi's address (tcp://<host>:9559) or a NaoqiRobotConfig
    /// JSON file.
    #[arg(env = "ROBOT")]
    robot: String,
    /// A JSON file overriding the robot's configuration.
    overrides: Option<String>,
    #[command(flatten)]
    runner: RunnerArgs,
}

impl Cli {
    /// The Groot XML of the tree the device runs: `--tree`'s file, none under
    /// `--no-tree`, else the default behavior.
    fn tree(&self) -> anyhow::Result<Option<String>> {
        if self.no_tree {
            return Ok(None);
        }
        match &self.tree {
            Some(path) => std::fs::read_to_string(path)
                .with_context(|| format!("failed to read the tree {}", path.display()))
                .map(Some),
            None => Ok(Some(READY_TREE.to_string())),
        }
    }
}

fn ready() -> StateChange {
    StateChange::set(COMMAND_READY, Value::String(READY_REQUEST.to_string()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let tree = cli.tree()?;
    let mut config = load_config(&cli.robot)?;
    if let Some(overrides) = &cli.overrides {
        config.apply_overrides(load_config(overrides)?);
    }

    // The front end first: it installs the log sink, so the connection's logs are shown.
    let (frontend, mut commands) = commands_frontend(vec![TuiCommand {
        key: READY_KEY,
        label: "say \"Arora is ready\"".to_string(),
        prompt: None,
    }]);
    // The modules the device carries, read before the robot is reached: a module directory
    // that does not read fails the start at once.
    let builder = Arora::builder()
        .with_frontend(frontend)
        .with_module_dirs(&arora::device_dir::from_env()?, &cli.runner.module)
        .await?;
    #[cfg(feature = "studio")]
    let builder = builder.with_studio(cli.runner.studio.choice()?);

    // One runtime carries everything: the HAL's tasks spawn on it at construction, the
    // voice speaks on it, and arora drives the device on it.
    let hal = NaoqiHal::new(config)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start the NAOqi HAL: {e}"))?;
    say::install(hal.voice());

    // The default behavior starts at once; the request key is open to remote writers, so
    // Studio or a bridge client can play it again as the terminal UI does.
    let store = SimpleDataStore::new();
    if tree.is_some() {
        let state = hal
            .read_all()
            .await
            .map_err(|e| anyhow::anyhow!("failed to read the robot's state: {e}"))?;
        store.write(ready_tree_initial_state(&state))?;
    }
    store.set_meta([(Key::from(COMMAND_READY), KeyMeta::new().editable())].into())?;
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

    let mut builder = builder
        .with_data_store(Box::new(store))
        .with_hal(Box::new(hal))
        .with_host_module(HostModule::of::<naoqi_say::Module>())
        .with_host_module(HostModule::of::<naoqi_gestures::Module>())
        .with_step_period(STEP_PERIOD);
    if let Some(tree) = tree {
        builder = builder.with_groot(tree);
    }
    builder.run().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    /// The robot, its overrides, the tree and `--module` in any order, the
    /// directory after a space or `=`.
    #[test]
    fn the_robot_the_tree_and_module_directories_parse() {
        let cli = Cli::try_parse_from([
            "arora-naoqi",
            "--module",
            "a",
            "--tree",
            "wave.groot.xml",
            "tcp://nao.local:9559",
            "--module=b",
            "overrides.json",
        ])
        .expect("the command line parses");
        assert_eq!(cli.robot, "tcp://nao.local:9559");
        assert_eq!(cli.overrides.as_deref(), Some("overrides.json"));
        assert_eq!(cli.tree, Some(PathBuf::from("wave.groot.xml")));
        assert_eq!(cli.runner.module, vec![PathBuf::from("a"), "b".into()]);
    }

    /// Without `--tree` the default behavior runs; `--no-tree` runs none.
    #[test]
    fn the_default_tree_runs_unless_none_is_asked() {
        let default = Cli::try_parse_from(["arora-naoqi", "tcp://nao.local:9559"]).unwrap();
        assert_eq!(default.tree().unwrap().as_deref(), Some(READY_TREE));
        let none =
            Cli::try_parse_from(["arora-naoqi", "--no-tree", "tcp://nao.local:9559"]).unwrap();
        assert_eq!(none.tree().unwrap(), None);
    }

    /// `--tree` and `--no-tree` together are refused.
    #[test]
    fn a_tree_and_no_tree_conflict() {
        let error = Cli::try_parse_from([
            "arora-naoqi",
            "--tree",
            "wave.groot.xml",
            "--no-tree",
            "tcp://nao.local:9559",
        ])
        .expect_err("the two are refused together");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    /// `-h` and `--help` print the help and exit 0; neither is a robot.
    #[test]
    fn help_is_not_a_robot() {
        for flag in ["-h", "--help"] {
            let error = Cli::try_parse_from(["arora-naoqi", flag]).expect_err("help stops");
            assert_eq!(error.kind(), ErrorKind::DisplayHelp, "{flag}");
            assert_eq!(error.exit_code(), 0, "{flag}");
        }
    }

    /// An option the runner does not take is refused, not read as a robot.
    #[test]
    fn an_unknown_option_is_refused() {
        let error = Cli::try_parse_from(["arora-naoqi", "--modules", "a", "tcp://nao.local:9559"])
            .expect_err("an unknown option is refused");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    /// `--studio` and `--no-studio` parse into the run's Studio choice,
    /// anywhere on the command line; neither leaves it to the saved
    /// credential.
    #[cfg(feature = "studio")]
    #[test]
    fn studio_and_no_studio_give_the_run_s_studio_choice() {
        use arora::studio::Choice;
        let choice = |args: &[&str]| {
            Cli::try_parse_from(args.iter().copied())
                .expect("the command line parses")
                .runner
                .studio
                .choice()
                .expect("ARORA_STUDIO is unset or a boolean")
        };
        assert_eq!(
            choice(&["arora-naoqi", "tcp://nao.local:9559"]),
            Choice::IfSaved
        );
        assert_eq!(
            choice(&["arora-naoqi", "--studio", "tcp://nao.local:9559"]),
            Choice::Studio
        );
        assert_eq!(
            choice(&["arora-naoqi", "tcp://nao.local:9559", "--no-studio"]),
            Choice::Local
        );
    }
}
