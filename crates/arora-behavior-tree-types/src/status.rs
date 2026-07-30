//! `Status` moved to [`arora_behavior`] — the cross-interpreter home, beside
//! `BehaviorStatus`/`TaskHandle` (ARORA-82). It is re-exported here so existing
//! consumers of `arora-behavior-tree-types` keep compiling unchanged; new code
//! links `arora-behavior` directly.
//!
//! The type, its value-plane conversions, the id constants, and
//! `declare_status_enumeration` are all defined in
//! [`arora_behavior::status`] — the ids are unchanged, so the wire form is
//! identical.

pub use arora_behavior::status::*;
