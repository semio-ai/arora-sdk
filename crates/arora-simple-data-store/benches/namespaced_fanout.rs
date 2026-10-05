//! What a frame costs when N devices share one store through `NamespacedStore`
//! views: each device writes K keys of its own, then every feed is drained —
//! one per device, plus one un-namespaced subscriber over the whole store.
//!
//! `cargo bench -p arora-simple-data-store --bench namespaced_fanout` runs a
//! grid of N and K; `-- N K` runs that one configuration.

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arora_simple_data_store::{NamespacedStore, SimpleDataStore};
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;

/// One frame: every device writes each of its keys to a new value, then every
/// feed is drained.
fn frame(
    devices: &[(NamespacedStore, Vec<Key>)],
    feeds: &[arora_types::data::Subscription],
    tick: f64,
) {
    for (view, keys) in devices {
        let change = StateChange {
            set: keys
                .iter()
                .map(|key| (key.clone(), Some(Value::F64(tick))))
                .collect(),
            unset: Default::default(),
        };
        view.write(change).unwrap();
    }
    for feed in feeds {
        for change in feed.try_iter() {
            black_box(change);
        }
    }
}

fn measure(n: usize, k: usize) -> Duration {
    let shared = SimpleDataStore::new();
    let inner: Arc<dyn DataStore> = Arc::new(shared.clone());
    let devices: Vec<(NamespacedStore, Vec<Key>)> = (0..n)
        .map(|d| {
            let view = NamespacedStore::new(inner.clone(), format!("device{d}"));
            let keys = (0..k).map(|i| Key::from(format!("track{i}"))).collect();
            (view, keys)
        })
        .collect();
    let mut feeds = vec![shared.subscribe()];
    feeds.extend(devices.iter().map(|(view, _)| view.subscribe()));

    let mut tick = 0.0;
    for _ in 0..10 {
        tick += 1.0;
        frame(&devices, &feeds, tick);
    }
    let frames = 100;
    let start = Instant::now();
    for _ in 0..frames {
        tick += 1.0;
        frame(&devices, &feeds, tick);
    }
    start.elapsed() / frames
}

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|arg| arg.parse().ok())
        .collect();
    let grid = match args[..] {
        [n, k] => vec![(n, k)],
        _ => [1, 10, 30]
            .into_iter()
            .flat_map(|n| [60, 600].map(|k| (n, k)))
            .collect(),
    };
    println!("devices  keys/device  per frame");
    for (n, k) in grid {
        println!("{n:>7}  {k:>11}  {:>9.1?}", measure(n, k));
    }
}
