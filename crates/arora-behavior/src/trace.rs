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
//! The report travels as a structured [`Value`] of flat arrays
//! ([`encode_report`], [`decode_report`]): a key-value whose fields are named,
//! each list of changes a few parallel arrays, a status or an activity a byte
//! ([`NodeStatus::code`], [`Activity::code`]), and a watched value as itself.

use std::sync::OnceLock;

use arora_types::keyvalue::{KeyValue, KeyValueField};
use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a trace follows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceOptions {
    /// The nodes whose output values the reports carry: their return value,
    /// kept by a binding or not, and the parameters they write. Empty, the
    /// reports carry statuses and detached activity only.
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

impl NodeStatus {
    /// The byte a report carries the status as: 0 success, 1 failure,
    /// 2 running, 3 not reached.
    pub fn code(self) -> u8 {
        match self {
            NodeStatus::Success => 0,
            NodeStatus::Failure => 1,
            NodeStatus::Running => 2,
            NodeStatus::NotReached => 3,
        }
    }

    /// The status a report's byte stands for.
    pub fn from_code(code: u8) -> Option<Self> {
        [
            NodeStatus::Success,
            NodeStatus::Failure,
            NodeStatus::Running,
            NodeStatus::NotReached,
        ]
        .get(usize::from(code))
        .copied()
    }
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

impl Activity {
    /// The byte a report carries the activity as: 0 called, 1 cached,
    /// 2 idle.
    pub fn code(self) -> u8 {
        match self {
            Activity::Called => 0,
            Activity::Cached => 1,
            Activity::Idle => 2,
        }
    }

    /// The activity a report's byte stands for.
    pub fn from_code(code: u8) -> Option<Self> {
        [Activity::Called, Activity::Cached, Activity::Idle]
            .get(usize::from(code))
            .copied()
    }
}

/// An output of a watched node, in a [`TraceReport`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortValue {
    /// The node.
    pub node: Uuid,
    /// The port: `_ret`'s id for the return value, a parameter's for a
    /// parameter the node writes.
    pub port: Uuid,
    /// Its new value; [`Value::Unit`] for an output that became unset.
    pub value: Value,
}

/// One report of a trace: what changed since the trace's previous report.
///
/// Each list holds only what changed, ordered by node (then port); an empty
/// one is left out of the encoded form. A trace's first report states every
/// node the tick reached, every detached node it read, and every watched
/// value that is set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TraceReport {
    /// The interpreter's tick the report was written at, counted from its
    /// first tick.
    pub tick: u64,
    /// The tick of the trace's previous report; `None` in its first. A
    /// reader whose last report is of another tick missed one, and starts a
    /// new trace for the whole picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    /// The nodes whose status changed, with their new status.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub status: Vec<(Uuid, NodeStatus)>,
    /// The detached nodes whose activity changed, with their new activity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity: Vec<(Uuid, Activity)>,
    /// The watched nodes' outputs that changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<PortValue>,
    /// The nodes an edit restarted since the previous report: each starts
    /// afresh, and this report states its status and values anew.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restarted: Vec<Uuid>,
    /// The behavior's revision
    /// ([`LoadedBehavior::revision`](crate::LoadedBehavior::revision)), when
    /// it changed since the trace's previous report, and in its first: a
    /// tracing client learns that the behavior was loaded or edited, by it
    /// or another client, in the report that follows the change. `None`
    /// when it did not change, or the interpreter keeps none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

impl TraceReport {
    /// Whether the report states no change.
    pub fn is_empty(&self) -> bool {
        self.status.is_empty()
            && self.activity.is_empty()
            && self.values.is_empty()
            && self.restarted.is_empty()
            && self.revision.is_none()
    }
}

/// The names of the encoded report's fields, in [`fields`]' order.
const NAMES: [&str; 11] = [
    "tick",
    "since",
    "status_nodes",
    "status",
    "activity_nodes",
    "activity",
    "value_nodes",
    "value_ports",
    "values",
    "restarted",
    "revision",
];

/// The encoded report's id and its fields' ids: the hashes of their names,
/// as [`arora_types::value_serde`] names a key-value and its entries.
fn fields() -> &'static (Uuid, [Uuid; 11]) {
    static IDS: OnceLock<(Uuid, [Uuid; 11])> = OnceLock::new();
    IDS.get_or_init(|| {
        (
            arora_types::gen_uuid_from_str("TraceReport"),
            NAMES.map(arora_types::gen_uuid_from_str),
        )
    })
}

/// A list of ids, as a report carries it: an array of uuid values.
fn ids(ids: impl Iterator<Item = Uuid>) -> Value {
    Value::ArrayValue(ids.map(Value::Uuid).collect())
}

/// The [`Value`] a trace writes to its feedback key: a key-value of the
/// fields the crate's `docs/trace.md` lists, each list of changes as
/// parallel arrays.
pub fn encode_report(report: &TraceReport) -> Value {
    let (id, field_ids) = fields();
    let mut encoded = KeyValue {
        id: *id,
        fields: Default::default(),
    };
    let mut set = |at: usize, value: Value| {
        encoded.fields.insert(
            NAMES[at].to_string(),
            KeyValueField {
                id: field_ids[at],
                name: NAMES[at].to_string(),
                value: Some(Box::new(value)),
            },
        );
    };
    set(0, Value::U64(report.tick));
    if let Some(since) = report.since {
        set(1, Value::U64(since));
    }
    if !report.status.is_empty() {
        set(2, ids(report.status.iter().map(|(node, _)| *node)));
        set(
            3,
            Value::ArrayU8(report.status.iter().map(|(_, s)| s.code()).collect()),
        );
    }
    if !report.activity.is_empty() {
        set(4, ids(report.activity.iter().map(|(node, _)| *node)));
        set(
            5,
            Value::ArrayU8(report.activity.iter().map(|(_, a)| a.code()).collect()),
        );
    }
    if !report.values.is_empty() {
        set(6, ids(report.values.iter().map(|value| value.node)));
        set(7, ids(report.values.iter().map(|value| value.port)));
        set(
            8,
            Value::ArrayValue(report.values.iter().map(|v| v.value.clone()).collect()),
        );
    }
    if !report.restarted.is_empty() {
        set(9, ids(report.restarted.iter().copied()));
    }
    if let Some(revision) = report.revision {
        set(10, Value::U64(revision));
    }
    Value::KeyValue(encoded)
}

/// Read a [`TraceReport`] out of a trace's feedback key.
pub fn decode_report(value: &Value) -> Result<TraceReport, String> {
    let Value::KeyValue(encoded) = value else {
        return Err(format!("a trace report is a key-value, not {value}"));
    };
    let field = |at: usize| -> Option<&Value> {
        encoded
            .fields
            .get(NAMES[at])
            .and_then(|field| field.value.as_deref())
    };
    let u64_at = |at: usize| match field(at) {
        Some(Value::U64(n)) => Ok(Some(*n)),
        None => Ok(None),
        Some(other) => Err(format!("{}: a u64, not {other}", NAMES[at])),
    };
    let ids_at = |at: usize| -> Result<Vec<Uuid>, String> {
        match field(at) {
            None => Ok(Vec::new()),
            Some(Value::ArrayValue(values)) => values
                .iter()
                .map(|value| match value {
                    Value::Uuid(id) => Ok(*id),
                    other => Err(format!("{}: a uuid, not {other}", NAMES[at])),
                })
                .collect(),
            Some(other) => Err(format!("{}: an array of uuids, not {other}", NAMES[at])),
        }
    };
    let bytes_at = |at: usize| -> Result<Vec<u8>, String> {
        match field(at) {
            None => Ok(Vec::new()),
            Some(Value::ArrayU8(bytes)) => Ok(bytes.clone()),
            Some(other) => Err(format!("{}: an array of u8, not {other}", NAMES[at])),
        }
    };
    let paired = |nodes: usize, codes: usize| -> Result<Vec<(Uuid, u8)>, String> {
        let (nodes_of, codes_of) = (ids_at(nodes)?, bytes_at(codes)?);
        if nodes_of.len() != codes_of.len() {
            return Err(format!(
                "{} and {} differ in length",
                NAMES[nodes], NAMES[codes]
            ));
        }
        Ok(nodes_of.into_iter().zip(codes_of).collect())
    };
    let status = paired(2, 3)?
        .into_iter()
        .map(|(node, code)| {
            NodeStatus::from_code(code)
                .map(|status| (node, status))
                .ok_or_else(|| format!("status: no status {code}"))
        })
        .collect::<Result<_, _>>()?;
    let activity = paired(4, 5)?
        .into_iter()
        .map(|(node, code)| {
            Activity::from_code(code)
                .map(|activity| (node, activity))
                .ok_or_else(|| format!("activity: no activity {code}"))
        })
        .collect::<Result<_, _>>()?;
    let (nodes, ports) = (ids_at(6)?, ids_at(7)?);
    let values = match field(8) {
        None => Vec::new(),
        Some(Value::ArrayValue(values)) => values.clone(),
        Some(other) => return Err(format!("values: an array, not {other}")),
    };
    if nodes.len() != values.len() || ports.len() != values.len() {
        return Err("value_nodes, value_ports and values differ in length".to_string());
    }
    Ok(TraceReport {
        tick: u64_at(0)?.ok_or("tick: missing")?,
        since: u64_at(1)?,
        status,
        activity,
        values: nodes
            .into_iter()
            .zip(ports)
            .zip(values)
            .map(|((node, port), value)| PortValue { node, port, value })
            .collect(),
        restarted: ids_at(9)?,
        revision: u64_at(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> TraceReport {
        TraceReport {
            tick: 7,
            since: Some(5),
            status: vec![
                (Uuid::from_u128(1), NodeStatus::Running),
                (Uuid::from_u128(2), NodeStatus::NotReached),
            ],
            activity: vec![(Uuid::from_u128(3), Activity::Cached)],
            values: vec![
                PortValue {
                    node: Uuid::from_u128(3),
                    port: Uuid::from_u128(4),
                    value: Value::F64(0.5),
                },
                PortValue {
                    node: Uuid::from_u128(3),
                    port: Uuid::from_u128(5),
                    value: Value::Unit,
                },
            ],
            restarted: vec![Uuid::from_u128(1)],
            revision: Some(3),
        }
    }

    #[test]
    fn a_report_round_trips_through_value() {
        let report = report();
        assert_eq!(decode_report(&encode_report(&report)).unwrap(), report);
        let empty = TraceReport::default();
        assert!(empty.is_empty());
        assert_eq!(decode_report(&encode_report(&empty)).unwrap(), empty);
        let revised = TraceReport {
            revision: Some(1),
            ..TraceReport::default()
        };
        assert!(!revised.is_empty(), "a new revision is a change");
        assert_eq!(decode_report(&encode_report(&revised)).unwrap(), revised);
    }

    /// The form a WebSocket client reads: the value vocabulary's JSON of a
    /// key-value of flat arrays, a watched value as itself.
    #[test]
    fn a_report_reaches_a_client_as_flat_arrays() {
        let json = serde_json::to_value(encode_report(&TraceReport {
            restarted: Vec::new(),
            since: None,
            revision: None,
            ..report()
        }))
        .unwrap();
        let fields = &json["keyvalue"]["fields"];
        let field = |name: &str| fields[name]["value"].clone();
        assert_eq!(field("tick"), serde_json::json!({"u64": 7}));
        assert_eq!(
            field("status_nodes"),
            serde_json::json!({"values": [
                {"uuid": "00000000-0000-0000-0000-000000000001"},
                {"uuid": "00000000-0000-0000-0000-000000000002"},
            ]})
        );
        assert_eq!(field("status"), serde_json::json!({"u8s": [2, 3]}));
        assert_eq!(field("activity"), serde_json::json!({"u8s": [1]}));
        assert_eq!(
            field("values"),
            serde_json::json!({"values": [{"f64": 0.5}, "unit"]})
        );
        assert!(fields.get("since").is_none() && fields.get("restarted").is_none());
        assert!(fields.get("revision").is_none());
        let json = serde_json::to_value(encode_report(&report())).unwrap();
        assert_eq!(
            json["keyvalue"]["fields"]["revision"]["value"],
            serde_json::json!({"u64": 3})
        );
    }

    #[test]
    fn codes_round_trip() {
        for status in [
            NodeStatus::Success,
            NodeStatus::Failure,
            NodeStatus::Running,
            NodeStatus::NotReached,
        ] {
            assert_eq!(NodeStatus::from_code(status.code()), Some(status));
        }
        for activity in [Activity::Called, Activity::Cached, Activity::Idle] {
            assert_eq!(Activity::from_code(activity.code()), Some(activity));
        }
        assert_eq!(NodeStatus::from_code(4), None);
    }

    #[test]
    fn a_malformed_report_is_refused() {
        let Value::KeyValue(mut encoded) = encode_report(&report()) else {
            unreachable!("a report is a key-value");
        };
        encoded.fields.remove("status");
        assert!(decode_report(&Value::KeyValue(encoded)).is_err());
        assert!(decode_report(&Value::U64(7)).is_err());
    }

    #[test]
    fn options_default_to_statuses_every_tick() {
        let options: TraceOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(options, TraceOptions::default());
        assert!(serde_json::from_str::<TraceOptions>(r#"{"watch": []}"#).is_err());
    }
}
