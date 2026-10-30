//! Traces: what a running behavior does, tick by tick, as a task run.
//!
//! A client starts a trace with
//! [`BehaviorInterpreter::trace`](crate::BehaviorInterpreter::trace) and
//! [`TraceOptions`]; the interpreter answers the [`TaskHandle`](crate::TaskHandle)
//! a spawned run answers. While the trace runs, the interpreter writes a
//! [`TraceReport`] to the handle's feedback key at the end of each tick that
//! changed something the trace follows, and [`halt`](crate::BehaviorInterpreter::halt)
//! ends it as it ends any run. The crate's `docs/trace.md` describes the
//! report and how an editor reads it.
//!
//! The report travels as a structured [`Value`], converted with
//! [`arora_types::value_serde`] ([`encode_report`], [`decode_report`]); its
//! serde form is its JSON form, so a browser reads the same field names.

use std::collections::BTreeMap;

use arora_types::value::Value;
use arora_types::value_serde;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a trace follows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceOptions {
    /// The nodes whose output values the reports carry: their return value
    /// and the parameters they write. Empty, the reports carry statuses and
    /// detached activity only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Uuid>,
    /// The least time between two reports, in nanoseconds on the device's
    /// timeline ([`built_in::TIME`](crate::built_in::TIME)). What changes in
    /// between is reported together, as it stands at the next report. `None`
    /// reports every tick that changed something.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_interval_ns: Option<u64>,
}

/// A node's status in a [`TraceReport`]: the [`Status`](crate::Status) it
/// returned when last ticked, or that the tick did not reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// The node succeeded.
    Success,
    /// The node failed, or its tick failed with an error.
    Failure,
    /// The node is running.
    Running,
    /// The tick did not reach the node.
    NotReached,
}

impl From<crate::Status> for NodeStatus {
    fn from(status: crate::Status) -> Self {
        match status {
            crate::Status::Success => NodeStatus::Success,
            crate::Status::Failure => NodeStatus::Failure,
            crate::Status::Running => NodeStatus::Running,
        }
    }
}

/// What a detached node did in a tick: a node no other node holds, evaluated
/// when what it outputs is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// It called its function: its arguments changed.
    Called,
    /// It was read, and answered from its last call: its arguments had not
    /// changed.
    Cached,
    /// Nothing read it.
    Idle,
}

/// One report of a trace: what changed since the trace's previous report.
///
/// Each field lists only what changed; an empty field is left out of the
/// serialized form. A trace's first report states every node the tick
/// reached, every detached node it read, and every watched value that is
/// set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TraceReport {
    /// The interpreter's tick the report was written at, counted from its
    /// first tick.
    pub tick: u64,
    /// The nodes whose status changed, with their new status.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub status: BTreeMap<Uuid, NodeStatus>,
    /// The detached nodes whose activity changed, with their new activity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub activity: BTreeMap<Uuid, Activity>,
    /// The watched nodes' outputs that changed, by node then by port: the
    /// return value under `_ret`'s id, a parameter the node writes under its
    /// own. An output that became unset reads [`Value::Unit`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<Uuid, BTreeMap<Uuid, Value>>,
    /// The nodes an edit restarted since the previous report: each starts
    /// afresh, and this report states its status and values anew.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restarted: Vec<Uuid>,
}

impl TraceReport {
    /// Whether the report states no change.
    pub fn is_empty(&self) -> bool {
        self.status.is_empty()
            && self.activity.is_empty()
            && self.values.is_empty()
            && self.restarted.is_empty()
    }
}

/// The [`Value`] a trace writes to its feedback key.
pub fn encode_report(report: &TraceReport) -> Value {
    value_serde::to_value(report).expect("a TraceReport converts to a Value")
}

/// Read a [`TraceReport`] out of a trace's feedback key.
pub fn decode_report(value: &Value) -> Result<TraceReport, String> {
    value_serde::from_value(value.clone()).map_err(|e| format!("malformed TraceReport: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> TraceReport {
        TraceReport {
            tick: 7,
            status: [
                (Uuid::from_u128(1), NodeStatus::Running),
                (Uuid::from_u128(2), NodeStatus::NotReached),
            ]
            .into(),
            activity: [(Uuid::from_u128(3), Activity::Cached)].into(),
            values: [(
                Uuid::from_u128(3),
                [(Uuid::from_u128(4), Value::F64(0.5))].into(),
            )]
            .into(),
            restarted: vec![Uuid::from_u128(1)],
        }
    }

    #[test]
    fn a_report_round_trips_through_value() {
        let report = report();
        assert_eq!(decode_report(&encode_report(&report)).unwrap(), report);
        let empty = TraceReport::default();
        assert!(empty.is_empty());
        assert_eq!(decode_report(&encode_report(&empty)).unwrap(), empty);
    }

    /// The JSON form a browser reads: names in snake case, ids as strings,
    /// empty fields left out.
    #[test]
    fn a_report_reads_as_json() {
        let json = serde_json::to_value(TraceReport {
            restarted: Vec::new(),
            ..report()
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "tick": 7,
                "status": {
                    "00000000-0000-0000-0000-000000000001": "running",
                    "00000000-0000-0000-0000-000000000002": "not_reached",
                },
                "activity": {"00000000-0000-0000-0000-000000000003": "cached"},
                "values": {
                    "00000000-0000-0000-0000-000000000003": {
                        "00000000-0000-0000-0000-000000000004": {"f64": 0.5},
                    },
                },
            })
        );
    }

    #[test]
    fn options_default_to_statuses_every_tick() {
        let options: TraceOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(options, TraceOptions::default());
        assert!(serde_json::from_str::<TraceOptions>(r#"{"watch": []}"#).is_err());
    }
}
