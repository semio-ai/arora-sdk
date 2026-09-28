//! `policy-device`: Microduck in MuJoCo on the Arora runtime.
//!
//! Without `--duration` the device serves at real time on the standard Arora
//! run: the operator front end (the terminal UI on a terminal), and the
//! bridge the build selects — Semio Studio when built with `--features
//! studio`, the open local bridge on `ws://127.0.0.1:9000` otherwise. With
//! `--duration`, the device runs that much simulated time in lockstep, as
//! fast as the machine allows, and prints what happened.

use std::path::PathBuf;

use anyhow::{Context, Result};
use arora_hal_mujoco::Clock;
use clap::Parser;
use policy_device::{
    default_model, serve, tree_path, Command, Device, DeviceConfig, Executor, Speech, StudioModel,
};

#[derive(Parser, Debug)]
#[command(about, version)]
struct Cli {
    /// The MJCF scene to simulate.
    #[arg(long, default_value_os_t = default_model())]
    model: PathBuf,
    /// The Groot behavior tree to run.
    #[arg(long, default_value_os_t = tree_path("interactive"))]
    tree: PathBuf,
    /// Where the modules run.
    #[arg(long, value_enum, default_value_t = Executor::Wasm)]
    executor: Executor,
    /// Run this much simulated time in lockstep and exit with a summary,
    /// instead of serving.
    #[arg(long)]
    duration: Option<f64>,
    /// Record joint positions, targets and the base position as CSV.
    #[arg(long)]
    record: Option<PathBuf>,
    /// The initial walking command: vx vy vyaw (m/s, m/s, rad/s).
    #[arg(long, num_args = 3, value_names = ["VX", "VY", "VYAW"], allow_negative_numbers = true)]
    command: Option<Vec<f64>>,
    /// The robot model saved by Semio Studio from the exported URDF
    /// (`tools/export-urdf.py`, then "Download Asset"): the joints and the
    /// base pose are published under the ids Studio drives it by.
    #[arg(long)]
    studio_model: Option<PathBuf>,
    /// Who speaks the tree's `Say` leaves: the cloud voice on this machine's
    /// speakers, or nobody (sentences are logged).
    #[arg(long, value_enum, default_value_t = Speech::Cloud)]
    speech: Speech,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let tree = std::fs::read_to_string(&cli.tree)
        .with_context(|| format!("could not read the tree {}", cli.tree.display()))?;
    let command = match &cli.command {
        Some(c) => Command {
            vx: c[0],
            vy: c[1],
            vyaw: c[2],
        },
        None => Command::default(),
    };
    let studio = cli
        .studio_model
        .as_deref()
        .map(StudioModel::from_glb)
        .transpose()?;
    let config = DeviceConfig {
        model: cli.model,
        tree,
        executor: cli.executor,
        clock: if cli.duration.is_some() {
            Clock::Lockstep
        } else {
            Clock::RealTime { speed: 1.0 }
        },
        record: cli.record,
        command,
        studio,
        speech: cli.speech,
    };

    match cli.duration {
        Some(seconds) => {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
                .init();
            let mut device = Device::build(config)?;
            let outcome = device.run_for(seconds)?;
            println!(
                "simulated {:.2} s: travelled {:.3} m to ({:.3}, {:.3}), height {:.3} m (min {:.3}), {}",
                outcome.simulated_seconds,
                outcome.distance,
                outcome.position[0],
                outcome.position[1],
                outcome.position[2],
                outcome.min_height,
                if outcome.fell { "FELL" } else { "upright" }
            );
            Ok(())
        }
        // The run installs its own log sink with the front end it picks.
        None => serve(config).await,
    }
}
