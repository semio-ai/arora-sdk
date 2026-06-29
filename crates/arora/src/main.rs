//! The default `arora` binary: run an arora instance with the in-process fake
//! HAL and bridge.
//!
//! Usage:
//!   arora [path/to/tree.groot.xml]
//!
//! This is the generic launcher. A device-specific build is a thin downstream
//! binary that depends on `arora` plus its own HAL/bridge crates and calls
//! [`arora::launch`] with those implementations — customization from the
//! outside, no feature flags inside `arora`.

use std::sync::Arc;

use anyhow::Result;
use arora_bridge::FakeBridge;
use arora_hal::FakeHal;
use arora_simple_data_store::SimpleDataStore;

fn main() -> Result<()> {
    arora::launch(
        Arc::new(FakeHal::new()),
        Arc::new(FakeBridge::new()),
        SimpleDataStore::new(),
    )
}
