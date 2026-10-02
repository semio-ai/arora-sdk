//! The default `arora` binary: the device runner, headless (no HAL-attached
//! display — a head like Vizij embeds [`arora::run_with_hal`] instead).
//!
//! It reads its configuration from the environment (the device directory,
//! Firebase options, Zenoh endpoints), loads the modules in its device
//! directory and those `--module` names, connects to Semio Studio over Zenoh
//! in a `studio-bridge` build (serving the open local bridge otherwise), and
//! runs the arora runtime. See [`arora::run_with_hal`] for the configuration
//! env vars and the full run.
//!
//! A device-specific build is a thin downstream binary that depends on `arora`
//! plus its own HAL/bridge crates and calls [`arora::run_with_hal`] /
//! [`arora::run_with`] with those implementations — customization from the
//! outside, no feature flags inside `arora`.

use std::collections::HashMap;
use std::rc::Rc;

use anyhow::Context;
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::DataStore;
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = arora::DeviceCli::parse();
    let store = SimpleDataStore::new();
    // `--open`: every key is an input. The empty prefix covers them all, and a
    // key's own meta can still close it again.
    if cli.open {
        store
            .set_prefix_meta(HashMap::from([(
                String::new(),
                arora_types::data::KeyMeta::new().editable(),
            )]))
            .context("the store keeps key meta")?;
    }
    // The front end first: building it installs the log sink, so what the
    // module loading below says — the directories it reads, a deprecated
    // variable it honours — is captured like the rest of the run.
    let mut builder = arora::Arora::builder()
        .with_data_store(Box::new(store))
        .with_frontend(arora::standard_frontend());
    // The modules the device carries: its device directory's, then `--module`'s.
    // Each loads at build, where one the engine rejects fails the start.
    for module in cli.modules()? {
        builder = builder.with_module(module.header, module.executable);
    }
    // A Groot file is a behavior-tree option: load it into a behavior-tree
    // interpreter against the store the device will tick. The tree binds no
    // module function (its function index is empty): its nodes are the
    // natively-hosted control nodes.
    if let Some(path) = cli.groot {
        let xml = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read Groot file {}", path.display()))?;
        let mut tree = arora::BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));
        tree.load_groot(&xml).map_err(|e| {
            anyhow::anyhow!(
                "failed to install behavior tree from {}: {e:?}",
                path.display()
            )
        })?;
        builder = builder.with_behavior_interpreter(Box::new(tree));
    }
    builder.run().await
}
