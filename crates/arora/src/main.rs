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
    // A Groot file is the device's behavior: the build resolves its tags
    // against the method index it assembles — the native nodes and the
    // functions of every module the device carries — and fails when the tree
    // does not load. A file that does not parse fails here, before the device
    // connects anywhere.
    if let Some(path) = cli.groot {
        let xml = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read Groot file {}", path.display()))?;
        arora_behavior_tree::schema_groot::BehaviorTree::try_from_groot_xml(&xml)
            .map_err(|e| anyhow::anyhow!("Groot file {} does not parse: {e:?}", path.display()))?;
        builder = builder.with_groot(xml);
    }
    builder.run().await
}
