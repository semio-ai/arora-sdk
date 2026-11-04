//! Built-in workload names shared by every harness.
use crate::{conversation, model::Workload, synthetic};
use serde_json::Value;

pub const NAMES: &[&str] = &["synthetic", "conversation"];
pub fn json(name: &str) -> Option<Value> {
    match name {
        "synthetic" => Some(synthetic::json()),
        "conversation" => Some(conversation::json()),
        _ => None,
    }
}
pub fn workload(name: &str) -> Option<Workload> {
    json(name).map(|j| Workload::parse(&j.to_string()).expect("valid built-in workload"))
}
