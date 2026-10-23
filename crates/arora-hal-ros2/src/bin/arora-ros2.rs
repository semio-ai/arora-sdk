//! arora-ros2: runs a ROS 2 robot as an Arora device.
//!
//! Usage: `arora-ros2 [--module <DIR>]... [--studio | --no-studio] <ROBOT>
//! [OVERRIDES]` (`--help` lists the options; `--studio` and `--no-studio` in a
//! build with the `studio` feature).
//!
//! The robot is picked at runtime: the first argument (or the `ROBOT` env var)
//! is a well-known robot name or a path to a `ROS2RobotConfig` JSON file. An
//! optional second JSON file overrides the selected config. `ROS_DOMAIN_ID`
//! overrides the config's domain id.
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
use arora_hal_ros2::{configs, ROS2RobotConfig, Ros2Hal};
use clap::Parser;

/// The runner's command line.
#[derive(Debug, Parser)]
#[command(name = "arora-ros2", about = "Runs a ROS 2 robot as an Arora device")]
struct Cli {
    /// The robot: a well-known name (nao, pepper, quori, ur3, ur5, g1) or a
    /// ROS2RobotConfig JSON file.
    #[arg(env = "ROBOT")]
    robot: String,
    /// A JSON file overriding the robot's configuration.
    overrides: Option<String>,
    #[command(flatten)]
    runner: RunnerArgs,
    /// `--studio` and `--no-studio`, under this crate's own `studio` feature:
    /// arora's `studio-bridge` may be enabled by another runner in the same
    /// build, and this one must still refuse `--studio` without its own.
    #[cfg(feature = "studio")]
    #[command(flatten)]
    studio: arora::studio::StudioArgs,
}

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
    let cli = Cli::parse();
    let mut config = load_config(&cli.robot);
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
        .with_module_dirs(&arora::device_dir::from_env()?, &cli.runner.module)
        .await?;
    #[cfg(feature = "studio")]
    let builder = builder.with_studio(cli.studio.choice()?);
    // One runtime carries everything: the HAL's ROS tasks spawn on it at
    // construction, and arora drives the device on it.
    let hal = Ros2Hal::new(config)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start the ROS 2 HAL: {e}"))?;
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

    /// The robot, its overrides and `--module` in any order, the directory
    /// after a space or `=`.
    #[test]
    fn the_robot_its_overrides_and_module_directories_parse() {
        let cli = Cli::try_parse_from([
            "arora-ros2",
            "--module",
            "a",
            "nao",
            "--module=b",
            "overrides.json",
        ])
        .expect("the command line parses");
        assert_eq!(cli.robot, "nao");
        assert_eq!(cli.overrides.as_deref(), Some("overrides.json"));
        assert_eq!(
            cli.runner.module,
            vec![std::path::PathBuf::from("a"), "b".into()]
        );
    }

    /// `-h` and `--help` print the help and exit 0; neither is a robot.
    #[test]
    fn help_is_not_a_robot() {
        for flag in ["-h", "--help"] {
            let error = Cli::try_parse_from(["arora-ros2", flag]).expect_err("help stops");
            assert_eq!(error.kind(), ErrorKind::DisplayHelp, "{flag}");
            assert_eq!(error.exit_code(), 0, "{flag}");
        }
    }

    /// An option the runner does not take is refused, not read as a robot.
    #[test]
    fn an_unknown_option_is_refused() {
        let error = Cli::try_parse_from(["arora-ros2", "--modules", "a", "nao"])
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
                .studio
                .choice()
                .expect("ARORA_STUDIO is unset or a boolean")
        };
        assert_eq!(choice(&["arora-ros2", "nao"]), Choice::Local);
        assert_eq!(choice(&["arora-ros2", "--studio", "nao"]), Choice::Studio);
        assert_eq!(choice(&["arora-ros2", "nao", "--no-studio"]), Choice::Local);
    }

    /// Built without the `studio` feature, the runner takes no Studio option:
    /// `--studio` is refused, not ignored.
    #[cfg(not(feature = "studio"))]
    #[test]
    fn studio_is_refused_without_the_studio_feature() {
        let error = Cli::try_parse_from(["arora-ros2", "--studio", "nao"])
            .expect_err("--studio is refused");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
}
