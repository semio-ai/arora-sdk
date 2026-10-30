//! Traces of the interpreter's ticks: what each tick did, recorded where the
//! tick does it, and reported to every trace that follows it.
//!
//! A lowered tree carries one [`TickRecord`]. While a trace runs, every node
//! that ticks records the status it returned, and every detached node that is
//! read records whether it called its function or answered from its cache.
//! With no trace running the record is off, and a tick costs one test of a
//! flag per node.
//!
//! At the end of a tick, [`Traces::report`] turns the record into what
//! changed since the previous tick — the nodes whose status changed, those the
//! tick reached before and not now, the detached nodes whose activity changed
//! — and hands that to each trace, which reports it at its own pace. The work
//! follows the nodes the tick reached and what changed, not the size of the
//! tree; a watched node's outputs are read only at a report. A watched node
//! records the value it returns when no binding keeps it.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use arora_behavior::trace::{encode_report, Activity, NodeStatus, TraceOptions, TraceReport};
use arora_behavior::{built_in, FunctionIndex, Status, TaskHandle};
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;
use uuid::Uuid;

use crate::id_hash::{IdMap, IdSet};
use crate::BehaviorTree;

/// What one tick of a lowered tree did, recorded while a trace runs.
#[derive(Default)]
pub(crate) struct TickRecord {
    /// Whether a trace runs: the nodes record only then.
    on: Cell<bool>,
    /// The nodes the tick reached, in tick order, with the status each
    /// returned. A node whose tick failed returned [`Status::Failure`].
    statuses: RefCell<Vec<(Uuid, Status)>>,
    /// The detached nodes the tick read, with what each did.
    activity: RefCell<Vec<(Uuid, Activity)>>,
    /// The nodes a trace watches.
    watched: RefCell<IdSet<Uuid>>,
    /// The watched nodes that returned a value no binding keeps, with that
    /// value; `None` for an optional return that holds nothing.
    returns: RefCell<Vec<(Uuid, Option<Value>)>>,
}

impl TickRecord {
    /// Whether the nodes record what they do.
    #[inline]
    pub(crate) fn is_on(&self) -> bool {
        self.on.get()
    }

    /// Record from now on, or stop; stopping forgets what was recorded.
    pub(crate) fn set_on(&self, on: bool) {
        self.on.set(on);
        if !on {
            self.statuses.borrow_mut().clear();
            self.activity.borrow_mut().clear();
            self.returns.borrow_mut().clear();
        }
    }

    /// Whether a trace watches node `node`.
    pub(crate) fn watches(&self, node: Uuid) -> bool {
        self.watched.borrow().contains(&node)
    }

    /// Watched node `node` returned `value`, which no binding keeps; its
    /// function's return is an optional when `optional`, which reads as a
    /// bound return's cell does: its value, or nothing. A status is no value:
    /// the node's status reports it.
    pub(crate) fn returned(&self, node: Uuid, value: &Value, optional: bool) {
        if Status::try_from(value.clone()).is_ok() {
            return;
        }
        let kept = match value {
            Value::Option(held) if optional => held.as_deref().cloned(),
            value => Some(value.clone()),
        };
        self.returns.borrow_mut().push((node, kept));
    }

    /// Node `node` returned `status`.
    pub(crate) fn status(&self, node: Uuid, status: Status) {
        self.statuses.borrow_mut().push((node, status));
    }

    /// Detached node `node` did `activity`.
    pub(crate) fn activity(&self, node: Uuid, activity: Activity) {
        self.activity.borrow_mut().push((node, activity));
    }
}

/// One running trace: its handle and options, and what its last report
/// stated.
struct Trace {
    handle: TaskHandle,
    options: TraceOptions,
    /// Whether its first report is written, its status key with it.
    started: bool,
    /// The device time of its last report, when the store had one.
    reported_at: Option<u64>,
    /// Each node's status as last reported; a node absent is not reached.
    status: IdMap<Uuid, NodeStatus>,
    /// Each detached node's activity as last reported; absent is idle.
    activity: IdMap<Uuid, Activity>,
    /// Each watched node's outputs as last reported, by port.
    values: IdMap<Uuid, IdMap<Uuid, Value>>,
    /// The nodes whose status may differ from the last report.
    status_changed: IdSet<Uuid>,
    /// The detached nodes whose activity may differ from the last report.
    activity_changed: IdSet<Uuid>,
    /// The nodes an edit restarted since the last report.
    restarted: Vec<Uuid>,
    /// The tick of its last report.
    last_tick: Option<u64>,
}

/// The interpreter's running traces, and what the previous tick did.
#[derive(Default)]
pub(crate) struct Traces {
    traces: Vec<Trace>,
    /// The nodes the previous tick reached, with their status.
    statuses: IdMap<Uuid, Status>,
    /// The detached nodes the previous tick read, with their activity.
    activity: IdMap<Uuid, Activity>,
    /// The maps the next tick is read into, kept so that a tick allocates
    /// none.
    spare: (IdMap<Uuid, Status>, IdMap<Uuid, Activity>),
    /// The nodes whose status, and the detached nodes whose activity,
    /// changed at this tick.
    delta: (Vec<Uuid>, Vec<Uuid>),
    /// What each watched node last returned that no binding keeps.
    returns: IdMap<Uuid, Value>,
}

impl Traces {
    /// Whether no trace runs.
    pub(crate) fn is_empty(&self) -> bool {
        self.traces.is_empty()
    }

    /// Start a trace under `options`, reported to `handle`'s keys.
    pub(crate) fn start(&mut self, handle: TaskHandle, options: TraceOptions) {
        self.traces.push(Trace {
            handle,
            options,
            started: false,
            reported_at: None,
            status: IdMap::default(),
            activity: IdMap::default(),
            values: IdMap::default(),
            status_changed: IdSet::default(),
            activity_changed: IdSet::default(),
            restarted: Vec::new(),
            last_tick: None,
        });
    }

    /// Set `record` to record while a trace runs, the watched nodes' returns
    /// included. A node no trace watches any more forgets what it returned:
    /// unrecorded, it would grow stale.
    pub(crate) fn arm(&mut self, record: &TickRecord) {
        record.set_on(!self.traces.is_empty());
        let mut watched = record.watched.borrow_mut();
        watched.clear();
        watched.extend(
            self.traces
                .iter()
                .flat_map(|trace| trace.options.values.iter().copied()),
        );
        self.returns.retain(|node, _| watched.contains(node));
    }

    /// End the trace `task` names, answering its status key; `None` when no
    /// trace has that id. With the last trace gone, what the previous tick
    /// did is forgotten: the next trace starts from its own first tick.
    pub(crate) fn halt(&mut self, task: arora_behavior::TaskId) -> Option<Key> {
        let at = self
            .traces
            .iter()
            .position(|trace| trace.handle.id == task)?;
        let trace = self.traces.remove(at);
        if self.traces.is_empty() {
            self.statuses.clear();
            self.activity.clear();
            self.returns.clear();
        }
        Some(trace.handle.status)
    }

    /// Forget what the nodes `tree` no longer has returned: a node set again
    /// under their id starts with no return.
    pub(crate) fn keep_returns_of(&mut self, tree: &BehaviorTree) {
        self.returns
            .retain(|node, _| tree.node_index.contains_key(node));
    }

    /// The nodes in `restarted` start afresh: each trace that has reported
    /// forgets what it reported of them, and its next report names them and
    /// states them anew.
    pub(crate) fn restart(&mut self, restarted: &[Uuid]) {
        for node in restarted {
            self.returns.remove(node);
        }
        for trace in self.traces.iter_mut().filter(|trace| trace.started) {
            for node in restarted {
                trace.status.remove(node);
                trace.activity.remove(node);
                trace.values.remove(node);
                trace.status_changed.insert(*node);
                trace.activity_changed.insert(*node);
            }
            trace.restarted.extend_from_slice(restarted);
        }
    }

    /// Take what tick `tick` did from `record`, and write each trace's report
    /// when it is due and states a change. `tree` holds the watched nodes'
    /// outputs, `functions` says which parameters a node writes.
    pub(crate) fn report(
        &mut self,
        tick: u64,
        record: &TickRecord,
        tree: Option<&BehaviorTree>,
        functions: &FunctionIndex,
        store: &dyn DataStore,
    ) -> Result<(), String> {
        let (mut statuses, mut activity) = std::mem::take(&mut self.spare);
        statuses.clear();
        statuses.extend(record.statuses.borrow_mut().drain(..));
        activity.clear();
        activity.extend(record.activity.borrow_mut().drain(..));
        for (node, returned) in record.returns.borrow_mut().drain(..) {
            match returned {
                Some(value) => self.returns.insert(node, value),
                None => self.returns.remove(&node),
            };
        }
        let (mut status_delta, mut activity_delta) = std::mem::take(&mut self.delta);
        changes(&self.statuses, &statuses, &mut status_delta);
        changes(&self.activity, &activity, &mut activity_delta);
        let now = self
            .traces
            .iter()
            .any(|trace| trace.options.min_interval_ns.is_some())
            .then(|| device_time(store))
            .flatten();
        let mut failure = None;
        for trace in &mut self.traces {
            if trace.started {
                trace.status_changed.extend(&status_delta);
                trace.activity_changed.extend(&activity_delta);
            } else {
                trace.status_changed.extend(statuses.keys());
                trace.activity_changed.extend(activity.keys());
            }
            let due = match (trace.options.min_interval_ns, trace.reported_at, now) {
                (Some(interval), Some(last), Some(now)) => now.saturating_sub(last) >= interval,
                _ => true,
            };
            if !due {
                continue;
            }
            let first = !trace.started;
            let report = trace.report(tick, &statuses, &activity, tree, functions, &self.returns);
            if !first && report.is_empty() {
                continue;
            }
            // A report that is not written leaves the trace to start over: its
            // next report states everything, as a first one does.
            if let Err(error) = trace.publish(&report, first, store) {
                trace.start_over();
                failure.get_or_insert(error);
                continue;
            }
            trace.started = true;
            trace.reported_at = now;
            trace.last_tick = Some(tick);
        }
        self.spare = (
            std::mem::replace(&mut self.statuses, statuses),
            std::mem::replace(&mut self.activity, activity),
        );
        self.delta = (status_delta, activity_delta);
        failure.map_or(Ok(()), Err)
    }
}

impl Trace {
    /// Write `report` to the feedback keys, and the trace's status first
    /// when it is the `first`.
    fn publish(
        &self,
        report: &TraceReport,
        first: bool,
        store: &dyn DataStore,
    ) -> Result<(), String> {
        if first {
            store
                .write(StateChange::set(
                    self.handle.status.clone(),
                    Status::Running.into(),
                ))
                .map_err(|e| format!("{}: writing a trace's status: {e}", self.handle.status))?;
        }
        for key in &self.handle.feedback {
            store
                .write(StateChange::set(key.clone(), encode_report(report)))
                .map_err(|e| format!("{key}: writing a trace's report: {e}"))?;
        }
        Ok(())
    }

    /// Forget everything reported: the next report is a first one.
    fn start_over(&mut self) {
        self.started = false;
        self.reported_at = None;
        self.last_tick = None;
        self.status.clear();
        self.activity.clear();
        self.values.clear();
        self.status_changed.clear();
        self.activity_changed.clear();
        self.restarted.clear();
    }

    /// The report of what changed since the last one, which it becomes.
    /// `returns` holds what the watched nodes last returned that no binding
    /// keeps.
    fn report(
        &mut self,
        tick: u64,
        statuses: &IdMap<Uuid, Status>,
        activity: &IdMap<Uuid, Activity>,
        tree: Option<&BehaviorTree>,
        functions: &FunctionIndex,
        returns: &IdMap<Uuid, Value>,
    ) -> TraceReport {
        let mut report = TraceReport {
            tick,
            since: self.last_tick,
            restarted: std::mem::take(&mut self.restarted),
            ..TraceReport::default()
        };
        for node in self.status_changed.drain() {
            let now = statuses
                .get(&node)
                .map_or(NodeStatus::NotReached, |status| NodeStatus::from(*status));
            if differs(&mut self.status, node, now, NodeStatus::NotReached) {
                report.status.insert(node, now);
            }
        }
        for node in self.activity_changed.drain() {
            let now = activity.get(&node).copied().unwrap_or(Activity::Idle);
            if differs(&mut self.activity, node, now, Activity::Idle) {
                report.activity.insert(node, now);
            }
        }
        if let Some(tree) = tree {
            for node in &self.options.values {
                let reported = self.values.entry(*node).or_default();
                let mut changed = BTreeMap::new();
                let outputs = tree.outputs_of(*node, functions, returns.get(node));
                for (port, value) in &outputs {
                    if reported.get(port) != Some(value) {
                        reported.insert(*port, value.clone());
                        changed.insert(*port, value.clone());
                    }
                }
                reported.retain(|port, _| {
                    let kept = outputs.iter().any(|(output, _)| output == port);
                    if !kept {
                        changed.insert(*port, Value::Unit);
                    }
                    kept
                });
                if !changed.is_empty() {
                    report.values.insert(*node, changed);
                }
            }
        }
        report
    }
}

/// Set `changed` to the keys of `before` and `after` whose entries differ, a
/// key one of them lacks included.
fn changes<T: PartialEq>(before: &IdMap<Uuid, T>, after: &IdMap<Uuid, T>, changed: &mut Vec<Uuid>) {
    changed.clear();
    changed.extend(
        after
            .iter()
            .filter(|(node, now)| before.get(*node) != Some(*now))
            .map(|(node, _)| *node),
    );
    changed.extend(before.keys().filter(|node| !after.contains_key(*node)));
}

/// Whether `now` differs from what `reported` holds for `node`, `absent`
/// when it holds nothing; `reported` takes `now`.
fn differs<T: Copy + PartialEq>(
    reported: &mut IdMap<Uuid, T>,
    node: Uuid,
    now: T,
    absent: T,
) -> bool {
    let before = if now == absent {
        reported.remove(&node)
    } else {
        reported.insert(node, now)
    };
    before.unwrap_or(absent) != now
}

/// The device's time, from its built-in key; `None` when the store has none.
fn device_time(store: &dyn DataStore) -> Option<u64> {
    match store.read(&[Key::from(built_in::TIME)]).pop().flatten() {
        Some(Value::U64(time)) => Some(time),
        _ => None,
    }
}
