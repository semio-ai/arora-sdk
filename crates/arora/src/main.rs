//! The `arora` binary: start the opinionated runtime, optionally run a Groot
//! behavior tree given at startup, then idle waiting for behavior trees over
//! the bridge.
//!
//! Usage:
//!   arora [path/to/tree.groot.xml]

use anyhow::{Context, Result};
use arora::Arora;

fn main() -> Result<()> {
    // An optional Groot behavior-tree XML file to run at startup.
    let groot_path = std::env::args().nth(1);

    // The async setup (loading the type records + the module) runs inside a
    // Tokio runtime. Ticking a tree, however, drives the wasm executor — which
    // manages its own blocking runtime — so that must happen OUTSIDE the
    // runtime below, after `block_on` returns.
    let runtime = tokio::runtime::Runtime::new().context("failed to start Tokio runtime")?;
    let mut arora = runtime.block_on(Arora::start())?;
    println!("arora: engine started; behavior-tree module loaded.");

    if let Some(path) = groot_path {
        let xml = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read Groot file {path}"))?;
        println!("arora: running behavior tree from {path}");
        let status = arora.run_groot_xml(&xml)?;
        println!("arora: behavior tree finished with status {status:?}");
    }

    println!("arora: idle — awaiting behavior trees over the bridge (Ctrl-C to stop).");
    arora.run_forever()
}
