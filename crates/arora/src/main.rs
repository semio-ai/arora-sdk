//! The default `arora` binary: the device runner, headless (no HAL-attached
//! display — a head like Vizij embeds [`arora::run_with_hal`] instead).
//!
//! It reads its configuration from the environment (Firebase options, Zenoh
//! endpoints, identity file), loads/saves an encrypted refresh token locally,
//! connects to Semio Studio over Zenoh, and runs the arora runtime. See
//! [`arora::run_with_hal`] for the configuration env vars and the full run.
//!
//! A device-specific build is a thin downstream binary that depends on `arora`
//! plus its own HAL/bridge crates and calls [`arora::run_with_hal`] /
//! [`arora::run_with`] with those implementations — customization from the
//! outside, no feature flags inside `arora`.

use std::collections::HashMap;
use std::rc::Rc;

use anyhow::Context;
use arora_simple_data_store::SimpleDataStore;
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = arora::DeviceCli::parse();
    let mut builder = arora::Arora::builder();
    // A Groot file is a behavior-tree option: load it into a behavior-tree
    // interpreter against the store the device will tick, and inject both.
    // This binary loads no host modules, so the function index is empty — the
    // tree's nodes are the natively-hosted control nodes.
    if let Some(path) = cli.groot {
        let xml = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read Groot file {}", path.display()))?;
        let store = SimpleDataStore::new();
        let mut tree = arora::BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));
        tree.load_groot(&xml, &store).map_err(|e| {
            anyhow::anyhow!(
                "failed to install behavior tree from {}: {e:?}",
                path.display()
            )
        })?;
        builder = builder
            .with_data_store(Box::new(store))
            .with_behavior_interpreter(Box::new(tree));
    }
    builder.run().await
}
