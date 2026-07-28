//! Task runs: concurrent, cancellable behaviors an interpreter hosts.
//!
//! A *task run* is a long-running unit of behavior started from a [`Call`] and
//! tracked by a serializable [`TaskHandle`]. It advances with the interpreter's
//! normal [`tick`](crate::BehaviorInterpreter::tick) — spawning one is
//! non-blocking — and everything an outside observer needs to follow or stop it
//! travels as data on the handle, so it can be driven through the value plane
//! without knowing how the interpreter hosts the run.

use arora_types::call::Call;
use arora_types::data::Key;
use uuid::Uuid;

/// Identity of a live task run.
///
/// Unique per run. It is also the namespacing root for the run's keys, which
/// live under `arora/tasks/<module>/<function>/<run_id>/…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub Uuid);

/// How a run coexists with the runs an interpreter already hosts.
///
/// Only [`Concurrent`](Self::Concurrent) is honoured today; the enum is
/// `#[non_exhaustive]` so conflict-resolution policies that need resource-claims
/// (preempt, queue, blend, reject) can be added later without a breaking change.
/// Arbitration, when those land, is the interpreter's — it is the only thing
/// that sees every run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunPolicy {
    /// Run alongside every other run. Overlapping actuation writes are
    /// last-write-wins — two runs driving the same output will fight.
    Concurrent,
}

/// The serializable contract for one run's lifecycle: everything needed to
/// follow and stop it, as data.
///
/// Returned from [`spawn`](crate::BehaviorInterpreter::spawn). The keys are
/// allocated by the interpreter under the run's [`id`](Self::id), so concurrent
/// runs demux. A run's *actuation* writes are deliberately not here — those go
/// to the shared standard keys, where overlapping runs are last-write-wins.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskHandle {
    /// The run's identity.
    pub id: TaskId,
    /// How to stop the run: a [`Call`] the observer issues, which reaches
    /// [`halt`](crate::BehaviorInterpreter::halt). Carrying it as a `Call` keeps
    /// "how to stop it" serializable and interpreter-agnostic.
    pub stop: Call,
    /// The run's lifecycle status key — a single key an observer watches to
    /// learn when the run reaches a terminal state and how it ended.
    pub status: Key,
    /// Keys carrying the run's progress feedback, updated as it runs.
    pub feedback: Vec<Key>,
    /// Keys carrying the run's result, written when it terminates.
    pub result: Vec<Key>,
    /// Keys an observer may write to steer a live run (e.g. a moving target).
    pub update: Vec<Key>,
}
