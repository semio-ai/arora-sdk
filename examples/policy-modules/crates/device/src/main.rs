//! `policy-device`: Microduck in MuJoCo on the Arora runtime.
//!
//! Without `--duration` the device serves at real time with the local bridge
//! open on `ws://127.0.0.1:9000`; with it, the device runs that much
//! simulated time in lockstep, as fast as the machine allows, and prints what
//! happened.

use std::path::PathBuf;

use anyhow::{Context, Result};
use arora_hal_mujoco::Clock;
use clap::Parser;
use policy_device::{default_model, tree_path, Command, Device, DeviceConfig, Executor};

#[derive(Parser, Debug)]
#[command(about, version)]
struct Cli {
    /// The MJCF scene to simulate.
    #[arg(long, default_value_os_t = default_model())]
    model: PathBuf,
    /// The Groot behavior tree to run.
    #[arg(long, default_value_os_t = tree_path("showcase"))]
    tree: PathBuf,
    /// Where the policy module runs.
    #[arg(long, value_enum, default_value_t = Executor::Wasm)]
    executor: Executor,
    /// Real-time factor when serving; 0 runs as fast as the machine allows.
    #[arg(long, default_value_t = 1.0)]
    speed: f64,
    /// Run this much simulated time in lockstep and exit with a summary,
    /// instead of serving.
    #[arg(long)]
    duration: Option<f64>,
    /// Record joint positions, targets and the base position as CSV.
    #[arg(long)]
    record: Option<PathBuf>,
    /// The walking command: vx vy vyaw (m/s, m/s, rad/s).
    #[arg(long, num_args = 3, value_names = ["VX", "VY", "VYAW"], allow_negative_numbers = true)]
    command: Option<Vec<f32>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
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
    let config = DeviceConfig {
        model: cli.model,
        tree,
        executor: cli.executor,
        clock: if cli.duration.is_some() {
            Clock::Lockstep
        } else {
            Clock::RealTime { speed: cli.speed }
        },
        record: cli.record,
        command,
    };

    match cli.duration {
        Some(seconds) => {
            let mut device = Device::build(config, None)?;
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
        None => {
            let bridge = arora::local_ws_bridge().await?;
            let device = Device::build(config, Some(bridge))?;
            log::info!(
                "serving; connect to ws://127.0.0.1:9000 and write command.vx / command.vy / command.vyaw to drive"
            );
            device.serve().await
        }
    }
}
