//! Print the declared header for the wasm executor as JSON.
//!
//! ```text
//! cargo run --offline -q -p arora-animation-module --example header
//! ```

use arora_animation_module::animation;
use arora_types::module::low::Executor;

fn main() {
    let header = animation::header(Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    });
    println!(
        "{}",
        serde_json::to_string(&header).expect("a header serializes to JSON")
    );
}
