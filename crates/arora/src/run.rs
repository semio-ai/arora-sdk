//! Running an arora: the crate's entry points.
//!
//! [`run`] is the whole story for the default device; the other entry points
//! peel away one default each. Every variant drives the same engine and
//! [`Runtime`] to completion (until the device is unregistered or the process
//! is interrupted), with an optional Groot tree queued from the first CLI
//! argument:
//!
//! - [`run`] — default HAL (in-process fake) and default bridge.
//! - [`run_with_hal`] — **your hardware**, default bridge. A device build is
//!   this one call: `arora::run_with_hal(Arc::new(MyHal::new()))`.
//! - [`run_with`] — your HAL, your bridge, your store. Full control.
//! - [`run_with_bridge_builder`] — like [`run_with`], for bridges whose
//!   construction is `async` and must live on arora's runtime (e.g. the Semio
//!   Studio connector).
//!
//! The **default bridge** depends on how the crate is built. By default it is
//! the open local bridge ([`arora-bridge-ws`](arora_bridge_ws)): the device
//! serves `ws://127.0.0.1:9000` and any editor or app on the machine connects
//! — no accounts. With the `studio-bridge` feature the device connects to
//! Semio Studio instead (Firebase auth + Zenoh). The two are mutually
//! exclusive: the runtime owns exactly one bridge.
//!
//! On the web, drive the runtime via `arora-web`'s `AroraRuntime` instead.

#[cfg(feature = "native")]
use std::sync::Arc;

#[cfg(feature = "native")]
use anyhow::{anyhow, Context, Result};
#[cfg(feature = "native")]
use arora_bridge::Bridge;
#[cfg(feature = "native")]
use arora_hal::Hal;
#[cfg(feature = "native")]
use arora_simple_data_store::SimpleDataStore;

#[cfg(feature = "native")]
use crate::runtime::Runtime;
#[cfg(feature = "native")]
use crate::Arora;

/// Run the default device: in-process fake HAL, default bridge.
#[cfg(feature = "native")]
pub fn run() -> Result<()> {
    run_with_hal(Arc::new(arora_hal::FakeHal::new()))
}

/// Run a device over `hal` with the default bridge — the one call that turns
/// a HAL into a running device.
#[cfg(all(feature = "native", not(feature = "studio-bridge")))]
pub fn run_with_hal(hal: Arc<dyn Hal>) -> Result<()> {
    let _ = env_logger::try_init();
    run_with_bridge_builder(hal, SimpleDataStore::new(), || async {
        let server = Arc::new(arora_bridge_ws::AroraWSServer::new(
            arora_bridge_ws::ServerConfig::default(),
        ));
        let bridge = arora_bridge_ws::bridge::WsBridge::new(server.clone()).await;
        tokio::spawn(async move {
            if let Err(e) = server.run(arora_bridge_ws::CancellationToken::new()).await {
                log::error!("local bridge server stopped: {e:?}");
            }
        });
        println!("arora: serving the local bridge on ws://127.0.0.1:9000");
        let bridge: Arc<dyn Bridge> = Arc::new(bridge);
        Ok(bridge)
    })
}

/// Run a device over `hal`, connected to Semio Studio (the `studio-bridge`
/// default bridge).
#[cfg(feature = "studio-bridge")]
pub fn run_with_hal(hal: Arc<dyn Hal>) -> Result<()> {
    crate::studio::run_with_hal(hal)
}

/// Run an arora instance with the given HAL, bridge, and data store.
///
/// Starts the engine (with the basic behavior-tree control nodes wired
/// natively), wires the portable [`Runtime`] around the injected HAL + bridge
/// over `store`, queues an optional Groot tree given as the first CLI
/// argument, spawns the asynchronous io pump on a Tokio runtime, then drives
/// the synchronous step loop on this thread.
///
/// Pass a freshly created [`SimpleDataStore`] for a self-contained device, or
/// a clone of a shared one to mutualize the blackboard across runtimes (e.g.
/// Studio handing one store to every spawned device).
#[cfg(feature = "native")]
pub fn run_with(hal: Arc<dyn Hal>, bridge: Arc<dyn Bridge>, store: SimpleDataStore) -> Result<()> {
    run_with_bridge_builder(hal, store, move || async move { Ok(bridge) })
}

/// Like [`run_with`], but constructs the bridge inside arora's Tokio runtime
/// via an asynchronous builder.
///
/// A bridge whose construction is `async` and whose background tasks must
/// live on the runtime that drives it — such as the studio-bridge connector's
/// `ZenohDeviceClient` — can't be built before this function creates its
/// runtime, and must not be built on a throwaway one it would outlive. This
/// variant runs the builder on arora's runtime, so the bridge and its tasks
/// share that runtime's lifetime.
#[cfg(feature = "native")]
pub fn run_with_bridge_builder<F, Fut>(
    hal: Arc<dyn Hal>,
    store: SimpleDataStore,
    make_bridge: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Arc<dyn Bridge>>>,
{
    // The async setup runs inside a Tokio runtime; the step loop that drives the
    // engine is synchronous and runs on this (main) thread afterwards — the wasm
    // executor manages its own blocking runtime and must not be ticked inside
    // Tokio.
    let tokio = tokio::runtime::Runtime::new().context("failed to start Tokio runtime")?;
    let (mut runtime, io) = tokio.block_on(async {
        let bridge = make_bridge().await.context("failed to build the bridge")?;
        let arora = Arora::start().await.context("failed to start Arora")?;
        // The public API takes a concrete `SimpleDataStore`; the runtime holds
        // `Arc<dyn DataStore>`, so wrap it here.
        let store: Arc<dyn arora_types::data::DataStore> = Arc::new(store);
        Ok::<_, anyhow::Error>(Runtime::with_io_in(arora, hal, bridge, store))
    })?;
    println!("arora: engine started; native behavior-tree control nodes ready.");

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
