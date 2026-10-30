//! arora-ros2: runs a ROS 2 robot as an Arora device.
//!
//! Usage: `arora-ros2 [--module <DIR>]... [--device <NAME>] [--device-dir <PATH>]
//! [--bridge-port <PORT>] [--no-studio] <ROBOT> [OVERRIDES]`
//! (`--help` lists the options; `--no-studio` in a build with the `studio`
//! feature).
//!
//! The robot is picked at runtime: the first argument (or the `ROBOT` env var)
//! is a well-known robot name or a path to a `ROS2RobotConfig` JSON file. An
//! optional second JSON file overrides the selected config. `ROS_DOMAIN_ID`
//! overrides the config's domain id.
//!
//! The device serves the open local bridge. Built with the `studio` feature, it
//! serves its Semio Studio bridge instead, unless the run passes
//! `--no-studio`: the device runs at once, and a device that is not enrolled
//! with Studio enrolls while it runs, as `arora::studio` says: the device
//! client enrolls it, presented by the run's front end, and the credential is
//! kept under the device's name.
//!
//! `--device`, `--device-dir` and `--bridge-port` (`ARORA_DEVICE`,
//! `ARORA_DEVICE_DIR`, `ARORA_BRIDGE_PORT`) give the device the run is: its
//! name, its directory and its local bridge's port, as `arora::Device` says.
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
    /// `--no-studio`, under this crate's own `studio` feature: arora's
    /// `studio-bridge` may be enabled by another runner in the same build, and
    /// this one, built without its own, neither connects nor takes the option.
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
    let frontend = arora::standard_frontend();
    #[cfg(feature = "studio")]
    let enroller = frontend.operator.clone().enroller();
    let builder = Arora::builder().with_frontend(frontend);
    let device = cli.runner.device()?;
    let builder = builder
        .with_module_dirs(device.dir(), &cli.runner.module)
        .await?;
    // One runtime carries everything: the HAL's ROS tasks spawn on it at
    // construction, and arora drives the device on it.
    let hal = Ros2Hal::new(config)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start the ROS 2 HAL: {e}"))?;
    // Built with `studio`, the run's bridge is the device's Studio bridge,
    // reporting the model the HAL describes, unless the run passes
    // `--no-studio`; the device enrolls beside the run, through the front
    // end's enroller, the enrollment's end failing the run.
    #[cfg(feature = "studio")]
    if cli.studio.connects() {
        use arora_hal::Hal;
        let model = arora::studio::reported_model(&hal.describe().await);
        let (bridge, handle) = arora::studio::of(&device)?.bridge(model);
        let run = builder
            .with_bridge(Box::new(bridge))
            .with_device(device)
            .with_hal(Box::new(hal))
            .run();
        return tokio::select! {
            ran = run => ran,
            ended = handle.enroll_when_needed(&*enroller) => Err(ended.into()),
        };
    }
    builder
        .with_device(device)
        .with_hal(Box::new(hal))
        .run()
        .await
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

    /// The device options give the device's name, directory and bridge port.
    #[test]
    fn the_device_options_give_the_device() {
        let cli = Cli::try_parse_from([
            "arora-ros2",
            "--device",
            "bench-2",
            "--device-dir=/srv/bench-2",
            "--bridge-port",
            "9002",
            "nao",
        ])
        .expect("the command line parses");
        let device = cli.runner.device().expect("the device resolves");
        assert_eq!(device.name(), "bench-2");
        assert_eq!(device.dir(), std::path::Path::new("/srv/bench-2"));
        assert_eq!(device.bridge_port(), 9002);
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

    /// Built with the `studio` feature, a run connects to Semio Studio unless
    /// it passes `--no-studio`, anywhere on the command line; `--studio` is no
    /// option.
    #[cfg(feature = "studio")]
    #[test]
    fn a_run_connects_to_studio_unless_it_passes_no_studio() {
        let connects = |args: &[&str]| {
            Cli::try_parse_from(args.iter().copied())
                .expect("the command line parses")
                .studio
                .connects()
        };
        assert!(connects(&["arora-ros2", "nao"]));
        assert!(!connects(&["arora-ros2", "--no-studio", "nao"]));
        assert!(!connects(&["arora-ros2", "nao", "--no-studio"]));
        let error = Cli::try_parse_from(["arora-ros2", "--studio", "nao"])
            .expect_err("--studio is refused");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    /// Built without the `studio` feature, the runner takes no Studio option:
    /// `--no-studio` is refused, not ignored.
    #[cfg(not(feature = "studio"))]
    #[test]
    fn no_studio_is_refused_without_the_studio_feature() {
        let error = Cli::try_parse_from(["arora-ros2", "--no-studio", "nao"])
            .expect_err("--no-studio is refused");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
}
