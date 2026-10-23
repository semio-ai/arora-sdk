//! arora-restful: runs an HTTP-controlled robot as an Arora device.
//!
//! Usage: `arora-restful [--module <DIR>]... [--studio | --no-studio] <CONFIG>
//! [OVERRIDES]` (`--help` lists the options; `--studio` and `--no-studio` in a
//! build with the `studio` feature).
//!
//! There are no built-in robots: the first argument (or the `CONFIG` env var)
//! is a path to a `RESTfulRobotConfig` JSON file describing the robot's HTTP
//! API (see `configs/hackerbot.json`). An optional second JSON file overrides
//! the selected config.
//!
//! The device serves the open local bridge. Built with the `studio` feature, it
//! connects to Semio Studio instead when the run passes `--studio`
//! (`ARORA_STUDIO=1`), which registers a device holding no saved Studio
//! credential; `--no-studio` keeps it on the local bridge. Its identity,
//! registration and credential are arora's (`arora::studio`), configured from
//! the environment.
//!
//! The device carries the modules its device directory holds under `modules/`,
//! then each `--module` directory, as the `arora` runner does
//! (`AroraBuilder::with_module_dirs`): a module directory is a module's
//! `header.json` beside its artifact, and optionally its records in
//! `records/`, which describe every function it exports.

use arora::{Arora, RunnerArgs};
use arora_hal_restful::{RESTfulRobotConfig, RestfulHal};
use clap::Parser;

/// The runner's command line.
#[derive(Debug, Parser)]
#[command(
    name = "arora-restful",
    about = "Runs an HTTP-controlled robot as an Arora device"
)]
struct Cli {
    /// The robot's RESTfulRobotConfig JSON file, describing its HTTP API.
    #[arg(env = "CONFIG")]
    config: String,
    /// A JSON file overriding the robot's configuration.
    overrides: Option<String>,
    #[command(flatten)]
    runner: RunnerArgs,
}

fn load_config(path: &str) -> RESTfulRobotConfig {
    let file = std::fs::File::open(path)
        .unwrap_or_else(|e| panic!("failed to open robot config {path}: {e}"));
    serde_json::from_reader(file)
        .unwrap_or_else(|e| panic!("failed to parse robot config {path}: {e}"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut config = load_config(&cli.config);
    if let Some(overrides_path) = &cli.overrides {
        config.apply_overrides(load_config(overrides_path));
    }

    // The front end first: building it installs the log sink, so what the
    // module loading below says is captured like the rest of the run.
    let builder = Arora::builder()
        .with_frontend(arora::standard_frontend())
        .with_module_dirs(&arora::device_dir::from_env()?, &cli.runner.module)
        .await?;
    #[cfg(feature = "studio")]
    let builder = builder.with_studio(cli.runner.studio.choice()?);
    let hal = RestfulHal::new(config)
        .map_err(|e| anyhow::anyhow!("failed to start the RESTful HAL: {e}"))?;
    builder.with_hal(Box::new(hal)).run().await
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

    /// The configuration, its overrides and `--module` in any order, the
    /// directory after a space or `=`.
    #[test]
    fn the_config_its_overrides_and_module_directories_parse() {
        let cli = Cli::try_parse_from([
            "arora-restful",
            "--module",
            "a",
            "hackerbot.json",
            "--module=b",
            "overrides.json",
        ])
        .expect("the command line parses");
        assert_eq!(cli.config, "hackerbot.json");
        assert_eq!(cli.overrides.as_deref(), Some("overrides.json"));
        assert_eq!(
            cli.runner.module,
            vec![std::path::PathBuf::from("a"), "b".into()]
        );
    }

    /// `-h` and `--help` print the help and exit 0; neither is a config file.
    #[test]
    fn help_is_not_a_config_file() {
        for flag in ["-h", "--help"] {
            let error = Cli::try_parse_from(["arora-restful", flag]).expect_err("help stops");
            assert_eq!(error.kind(), ErrorKind::DisplayHelp, "{flag}");
            assert_eq!(error.exit_code(), 0, "{flag}");
        }
    }

    /// An option the runner does not take is refused, not read as a config
    /// file.
    #[test]
    fn an_unknown_option_is_refused() {
        let error = Cli::try_parse_from(["arora-restful", "--modules", "a", "hackerbot.json"])
            .expect_err("an unknown option is refused");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    /// `--studio` and `--no-studio` parse into the run's Studio choice,
    /// anywhere on the command line; neither serves the open local bridge.
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
        assert_eq!(choice(&["arora-restful", "hackerbot.json"]), Choice::Local);
        assert_eq!(
            choice(&["arora-restful", "--studio", "hackerbot.json"]),
            Choice::Studio
        );
        assert_eq!(
            choice(&["arora-restful", "hackerbot.json", "--no-studio"]),
            Choice::Local
        );
    }

    /// Built without the `studio` feature, the runner takes no Studio option:
    /// `--studio` is refused, not ignored.
    #[cfg(not(feature = "studio"))]
    #[test]
    fn studio_is_refused_without_the_studio_feature() {
        let error = Cli::try_parse_from(["arora-restful", "--studio", "hackerbot.json"])
            .expect_err("--studio is refused");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
}
