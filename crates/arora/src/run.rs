//! Running an arora: the crate's entry points.
//!
//! Every entry point is sugar over the [builder](Arora::builder): it assembles
//! an [`AroraBuilder`](crate::AroraBuilder) and calls
//! [`run`](crate::AroraBuilder::run), which drives the device to completion
//! (until the device is unregistered or the process is interrupted). The
//! [`DeviceCli`] helper gives a binary the standard device arguments to
//! parse. A device that composes its own parts (a custom store, several
//! bridges, modules) skips this module and uses the builder directly. All entry points are `async` —
//! the caller drives them on its own Tokio runtime (the binary from
//! `#[tokio::main]`) — and differ only in which seams the caller supplies vs.
//! defaults:
//!
//! - [`run`] — default HAL (in-process fake) and default bridge.
//! - [`run_with_hal`] — **your hardware**, default bridge. A device build is
//!   this one call: `arora::run_with_hal(Box::new(MyHal::new())).await`.
//! - [`run_with`] — your HAL, your bridge, your store, each owned by the
//!   device. Full control; the caller builds the bridge (awaiting its async
//!   construction itself) and hands it in.
//!
//! The **default bridge** depends on how the crate is built. By default it is
//! the open local bridge ([`arora-bridge-ws`](arora_bridge_ws)): the device
//! serves `ws://127.0.0.1:9000` and any editor or app on the machine connects
//! — no accounts. With the `studio-bridge` feature the device connects to
//! Semio Studio instead (Firebase auth + Zenoh). The two are mutually
//! exclusive: each of these entry points wires exactly one bridge. (Assembling
//! an [`Arora`] directly with the builder can wire several.)
//!
//! On the web, drive the device via `arora-web`'s `AroraRuntime` instead.

#[cfg(feature = "native")]
use std::sync::Arc;

#[cfg(feature = "native")]
use anyhow::{anyhow, Context, Result};
#[cfg(feature = "native")]
use arora_bridge::{AccessRequestStream, Bridge, BridgeResult, DeviceInfo, InboundStream};
#[cfg(feature = "native")]
use arora_hal::Hal;
#[cfg(feature = "native")]
use arora_types::data::DataStore;
#[cfg(feature = "native")]
use arora_types::data::StateChange;
#[cfg(feature = "native")]
use futures::FutureExt;
#[cfg(feature = "native")]
use log::info;

#[cfg(feature = "native")]
use crate::operator::{serve_access_requests, Frontend};
#[cfg(feature = "native")]
use crate::Arora;

/// The standard device binary's command line, as a clap helper: parse it in
/// your `main` (or `#[command(flatten)]` it into a larger CLI) and inject the
/// results through the builder's seams.
///
/// The Groot argument is a behavior-tree option: it loads into a
/// [`BehaviorTreeInterpreter`](crate::BehaviorTreeInterpreter)
/// (`load_groot`, against the store the device will tick) injected via
/// [`with_behavior_interpreter`](crate::AroraBuilder::with_behavior_interpreter)
/// — the `arora` binary's `main` is the worked example.
#[cfg(feature = "native")]
#[derive(Debug, Default, clap::Parser)]
pub struct DeviceCli {
    /// Groot behavior-tree file to install as the device's behavior.
    pub groot: Option<std::path::PathBuf>,
}

/// Run the default device: in-process fake HAL, default bridge.
#[cfg(feature = "native")]
pub async fn run() -> Result<()> {
    Arora::builder().run().await
}

/// Run a device over `hal` with the default bridge — the one call that turns
/// a HAL into a running device. Sugar for
/// `Arora::builder().with_hal(hal).run()`; use the builder directly to inject
/// any other part (a custom [`DataStore`], extra bridges, modules, …).
#[cfg(feature = "native")]
pub async fn run_with_hal(hal: Box<dyn Hal>) -> Result<()> {
    Arora::builder().with_hal(hal).run().await
}

/// Build (and start serving) the open local bridge — the device serves
/// `ws://127.0.0.1:9000` and any editor or app on the machine connects, no
/// accounts. This is the default-build bridge; the `studio-bridge` build also
/// falls back to it when the operator declines a Studio connection, so a device
/// without an owner still exposes a local bridge (just no Semio Studio).
///
/// [`AroraBuilder::run`](crate::AroraBuilder::run) attaches this bridge for you
/// when you inject none. A host that composes its own bridge set — say the open
/// local bridge *and* a ROS 2 bridge — attaches it explicitly instead, the same
/// way [`studio::connect`](crate::studio::connect) is composed:
///
/// ```ignore
/// let device = arora::Arora::builder()
///     .with_bridge(arora::local_ws_bridge().await?)
///     .with_bridge(ros2_bridge)
///     .run()
///     .await?;
/// ```
///
/// The server it starts is cancelled when the returned bridge is dropped, so the
/// port frees for the next device in the same process.
#[cfg(feature = "native")]
pub async fn local_ws_bridge() -> Result<Box<dyn Bridge>> {
    let server = Arc::new(arora_bridge_ws::AroraWSServer::new(
        arora_bridge_ws::ServerConfig::default(),
    ));
    let bridge = arora_bridge_ws::bridge::WsBridge::new(server.clone()).await;
    // Bind before spawning: an unusable address (port already taken) fails the
    // run here instead of leaving a device serving a bridge nobody can reach.
    let listener = server
        .bind()
        .await
        .map_err(|e| anyhow!("local bridge: {e}"))?;
    let cancel = arora_bridge_ws::CancellationToken::new();
    tokio::spawn({
        let cancel = cancel.clone();
        async move {
            if let Err(e) = server.run_on(listener, cancel).await {
                log::error!("local bridge server stopped: {e:?}");
            }
        }
    });
    info!("serving the local bridge on ws://127.0.0.1:9000");
    Ok(Box::new(LocalBridge {
        inner: bridge,
        server: cancel,
    }))
}

/// The open local bridge with its server's lifetime attached: dropping the
/// bridge — the device that owned it ending its run — cancels the serving
/// task, so the port is free for the next device in the same process.
#[cfg(feature = "native")]
struct LocalBridge {
    inner: arora_bridge_ws::bridge::WsBridge,
    server: arora_bridge_ws::CancellationToken,
}

#[cfg(feature = "native")]
impl Drop for LocalBridge {
    fn drop(&mut self) {
        self.server.cancel();
    }
}

#[cfg(feature = "native")]
#[async_trait::async_trait]
impl Bridge for LocalBridge {
    fn take_inbound(&mut self) -> InboundStream {
        self.inner.take_inbound()
    }

    fn try_send(&mut self, change: &StateChange) {
        self.inner.try_send(change)
    }

    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
        self.inner.get_device_info().await
    }

    async fn update_device_info(
        &self,
        info: Option<DeviceInfo>,
    ) -> BridgeResult<Option<DeviceInfo>> {
        self.inner.update_device_info(info).await
    }

    async fn device_id(&self) -> Option<String> {
        self.inner.device_id().await
    }

    async fn access_requests(&self) -> AccessRequestStream {
        self.inner.access_requests().await
    }
}

/// Run an arora device with the given HAL, bridge, and data store.
///
/// Builds an [`Arora`] (engine with the basic behavior-tree control nodes wired
/// natively) around the injected HAL + bridge over `store`, then drives the
/// step loop. There is no bridge factory — the caller builds the bridge
/// endpoint (awaiting any async construction on its own runtime) and hands it
/// in here **by value**: the device owns it, and takes its inbound stream at
/// build. The bridge and HAL own any async internally.
///
/// Pass `Box::new(SimpleDataStore::new())` for a self-contained device, or a
/// clone onto shared storage (any [`DataStore`] — e.g. a `NamespacedStore`
/// over one mutualized backend) to share the blackboard across devices.
#[cfg(feature = "native")]
pub async fn run_with(
    hal: Box<dyn Hal>,
    bridge: Box<dyn Bridge>,
    store: Box<dyn DataStore>,
) -> Result<()> {
    Arora::builder()
        .with_hal(hal)
        .with_bridge(bridge)
        .with_data_store(store)
        .run()
        .await
}

/// Like [`run_with`], but with a caller-supplied [`Frontend`] — the operator that
/// answers the device's questions and the log sink that goes with it.
///
/// This is the seam every other entry point funnels through to pick between the
/// terminal operator UI and the headless front end; a device build with its own
/// UI supplies its own [`Frontend`] here. The rest of the run family picks one
/// itself: the terminal UI when the process is attached to a terminal, headless
/// otherwise.
#[cfg(feature = "native")]
pub async fn run_with_frontend(
    hal: Box<dyn Hal>,
    bridge: Box<dyn Bridge>,
    store: Box<dyn DataStore>,
    frontend: Frontend,
) -> Result<()> {
    run_builder_with_frontend(
        Arora::builder()
            .with_hal(hal)
            .with_bridge(bridge)
            .with_data_store(store),
        frontend,
    )
    .await
}

/// The run loop over a fully-assembled [`AroraBuilder`] — the funnel every
/// entry point (and [`AroraBuilder::run`]) goes through. Expects at least one
/// bridge to be injected already; every other unset part gets its default at
/// `build()`.
#[cfg(feature = "native")]
pub(crate) async fn run_builder_with_frontend(
    mut builder: crate::AroraBuilder,
    frontend: Frontend,
) -> Result<()> {
    let Frontend {
        operator, on_ready, ..
    } = frontend;

    // Query the bridge's control plane before the device takes ownership of the
    // endpoint: the identity/info the front end shows, and the access-request
    // stream the operator serves for the rest of the run. Multi-bridge devices
    // expose the first (default) bridge's control plane to the front end.
    let bridge = builder
        .bridges
        .first_mut()
        .ok_or_else(|| anyhow!("no bridge injected"))?;
    let info = bridge.get_device_info().await.ok().flatten();
    let device_id = bridge.device_id().await;
    let access_requests = bridge.access_requests().await;

    let mut arora = builder.build().context("failed to build Arora")?;

    // Hand the front end its live view now that the device exists: a
    // subscription opening on the device's whole state, and its identity.
    on_ready(arora.store().subscribe(), info, device_id);

    info!("engine started; native behavior-tree control nodes ready");

    // Serve remote clients' access requests through the chosen operator, one
    // at a time, for as long as the bridge yields them, concurrently with the
    // step loop — in this same future, not on a spawned task: everything the
    // run holds (the front end via the operator, the device, its bridges)
    // lives in this scope, so dropping the returned future is a complete,
    // synchronous teardown. That is the stop story — see [`AroraBuilder::run`].
    let serving = serve_access_requests(access_requests, operator).fuse();
    futures::pin_mut!(serving);
    info!("running — Ctrl-C to stop");
    let run = arora.run(Arora::DEFAULT_STEP_PERIOD).fuse();
    futures::pin_mut!(run);
    loop {
        futures::select_biased! {
            result = run => return result.map_err(|e| anyhow!("runtime error: {e}")),
            // The access-request stream ending does not end the device.
            _ = serving => {}
        }
    }
}

/// Pick the front end for this process: the terminal operator UI when the `tui`
/// feature is on and stdout is a terminal, otherwise the headless front end.
///
/// Building the front end installs the matching log sink, so the run path calls
/// this before it emits any logs it wants captured.
#[cfg(feature = "native")]
pub(crate) fn select_frontend() -> Frontend {
    #[cfg(feature = "tui")]
    {
        use std::io::IsTerminal;
        if std::io::stdout().is_terminal() {
            match crate::tui::tui_frontend() {
                Ok(frontend) => return frontend,
                Err(e) => eprintln!("arora: terminal UI unavailable ({e}); running headless"),
            }
        }
    }
    crate::operator::default_frontend()
}
