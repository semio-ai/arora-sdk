//! The `arora` binary: start the opinionated runtime with a HAL and a bridge,
//! optionally queue a Groot behavior tree at startup, then drive the runtime
//! loop (bridge commands, HAL sensors, behavior-tree ticks) until the device is
//! unregistered or the process is interrupted.
//!
//! Usage:
//!   arora [path/to/tree.groot.xml]
//!
//! This is the launcher that supersedes studio-bridge's `headless`. For now it
//! wires the in-process fakes ([`FakeHal`] + [`FakeBridge`]); the rest of the
//! headless functionality (feature-gated robot HALs, the studio-bridge
//! connector, config / token storage / device-info sync) is migrated on top of
//! this foundation.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use arora::runtime::Runtime;
use arora::Arora;
use arora_bridge::FakeBridge;
use arora_hal::FakeHal;

fn main() -> Result<()> {
    // An optional Groot behavior-tree XML file to run at startup.
    let groot_path = std::env::args().nth(1);

    // The async setup (loading the type records + the module, taking the
    // bridge/HAL streams) runs inside a Tokio runtime. The step loop that drives
    // the engine is synchronous and runs on this (main) thread afterwards — the
    // wasm executor manages its own blocking runtime and must not be ticked
    // inside Tokio.
    let tokio = tokio::runtime::Runtime::new().context("failed to start Tokio runtime")?;
    let (mut runtime, io) = tokio.block_on(async {
        let arora = Arora::start().await.context("failed to start Arora")?;
        Ok::<_, anyhow::Error>(Runtime::with_io(
            arora,
            Arc::new(FakeHal::new()),
            Arc::new(FakeBridge::new()),
        ))
    })?;
    println!("arora: engine started; behavior-tree module loaded.");

    if let Some(path) = groot_path {
        let xml = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read Groot file {path}"))?;
        runtime
            .queue_groot_xml(&xml)
            .map_err(|e| anyhow!("failed to queue behavior tree from {path}: {e}"))?;
        println!("arora: queued behavior tree from {path}");
    }

    // The asynchronous bridge/HAL io pump runs on the Tokio runtime; the
    // synchronous step loop drives the engine here until the device is
    // unregistered (Ctrl-C otherwise stops the process).
    tokio.spawn(io);
    println!("arora: running — Ctrl-C to stop.");
    runtime.run().map_err(|e| anyhow!("runtime error: {e}"))
}
