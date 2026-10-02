//! A key as a client sees it.

use arora_types::data::KeyMeta;
use serde::{Deserialize, Serialize};

/// A key of the device: its path, and what the store says it is.
///
/// The meta is the store's answer, relayed — the shape the key holds, the range
/// it runs over and the unit it is counted in, where it rests, what it is for,
/// and whether a client may write it. A key nobody has described carries the
/// default: the shape of the value it holds, and closed to writes.
///
/// On the wire the meta is `__meta`, the name every Arora bridge gives it, so a
/// key segment called `meta` never collides with it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyInfo {
    /// Hierarchical path identifier (e.g. `face/mouth/open`).
    pub path: String,

    /// What the store says this key is.
    #[serde(rename = "__meta")]
    pub meta: KeyMeta,
}
