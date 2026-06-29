//! The arora launcher — the reusable entry point a device-specific build calls
//! to run an arora instance with **its own** HAL and bridge.
//!
//! Customization happens from the *outside*: rather than `arora` carrying a
//! feature flag per robot, a device-specific binary depends on `arora` plus its
//! custom HAL (and bridge) crates and calls [`launch`]:
//!
//! ```no_run
//! # use std::sync::Arc;
//! # #[cfg(feature = "native")]
//! # fn main() -> anyhow::Result<()> {
//! // a hypothetical `arora-ur5` binary:
//! arora::launch(Arc::new(my_hal::Ur5Hal::new()), Arc::new(my_bridge::Studio::new()))
//! # }
//! # #[cfg(not(feature = "native"))] fn main() {}
//! # mod my_hal { pub struct Ur5Hal; impl Ur5Hal { pub fn new() -> arora_hal::FakeHal { arora_hal::FakeHal::new() } } }
//! # mod my_bridge { pub struct Studio; impl Studio { pub fn new() -> arora_bridge::FakeBridge { arora_bridge::FakeBridge::new() } } }
//! ```
//!
//! The default `arora` binary calls this with the in-process fakes. The launcher
//! owns the parts every device shares (engine startup, the run loop, and — as
//! they are migrated from studio-bridge's `headless` — CLI/env, config, token
//! storage and device-info sync); the device-specific binary only injects the
//! HAL and bridge implementations.

#[cfg(feature = "native")]
use std::sync::Arc;

#[cfg(feature = "native")]
use anyhow::{anyhow, Context, Result};
#[cfg(feature = "native")]
use arora_bridge::Bridge;
#[cfg(feature = "native")]
use arora_hal::Hal;

#[cfg(feature = "native")]
use crate::runtime::Runtime;
#[cfg(feature = "native")]
use crate::Arora;

/// Run an arora instance with the given HAL and bridge until the device is
/// unregistered (or the process is interrupted).
///
/// Starts the engine (with the embedded behavior-tree module), wires the
/// portable [`Runtime`] around the injected HAL + bridge, queues an optional
/// Groot tree given as the first CLI argument, spawns the asynchronous io pump
/// on a Tokio runtime, then drives the synchronous step loop on this thread.
///
/// This is the native launcher; on the web, drive the runtime via
/// `arora-web`'s `AroraRuntime` instead.
#[cfg(feature = "native")]
pub fn launch(hal: Arc<dyn Hal>, bridge: Arc<dyn Bridge>) -> Result<()> {
    // The async setup runs inside a Tokio runtime; the step loop that drives the
    // engine is synchronous and runs on this (main) thread afterwards — the wasm
    // executor manages its own blocking runtime and must not be ticked inside
    // Tokio.
    let tokio = tokio::runtime::Runtime::new().context("failed to start Tokio runtime")?;
    let (mut runtime, io) = tokio.block_on(async {
        let arora = Arora::start().await.context("failed to start Arora")?;
        Ok::<_, anyhow::Error>(Runtime::with_io(arora, hal, bridge))
    })?;
    println!("arora: engine started; behavior-tree module loaded.");

    if let Some(path) = std::env::args().nth(1) {
        let xml = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read Groot file {path}"))?;
        runtime
            .queue_groot_xml(&xml)
            .map_err(|e| anyhow!("failed to queue behavior tree from {path}: {e}"))?;
        println!("arora: queued behavior tree from {path}");
    }

    tokio.spawn(io);
    println!("arora: running — Ctrl-C to stop.");
    runtime.run().map_err(|e| anyhow!("runtime error: {e}"))
}
