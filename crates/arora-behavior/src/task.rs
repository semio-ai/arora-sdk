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

/// A [`TaskId`] travels on the value plane as a bare scalar uuid — its arora
/// type is the well-known uuid primitive. The distinct Rust newtype conveys
/// that a uuid *is* a task's identity while carrying no schema of its own.
impl arora_types::AroraType for TaskId {
    fn arora_type_id() -> Uuid {
        *arora_types::ty::UUID_ID
    }

    fn arora_type() -> arora_types::ty::low::Type {
        arora_types::ty::PRIMITIVE_TYPES
            .get(&*arora_types::ty::UUID_ID)
            .expect("the uuid primitive is always registered")
            .clone()
    }

    fn register_types(_registry: &mut arora_types::ty::TypeRegistry) {
        // A uuid is a well-known primitive, resolved without the registry.
    }
}

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
#[derive(Debug, Clone, PartialEq, arora_types::AroraType)]
#[arora(id = "d1f79433-a751-4fe3-9e6f-561cea873293")]
pub struct TaskHandle {
    /// The run's identity.
    #[arora(id = "35a14602-9fb2-4387-b202-21f0fc8cfb0c")]
    pub id: TaskId,
    /// How to stop the run: a [`Call`] the observer issues, which reaches
    /// [`halt`](crate::BehaviorInterpreter::halt). Carrying it as a `Call` keeps
    /// "how to stop it" serializable and interpreter-agnostic.
    #[arora(id = "41e5efbb-959a-4aa0-b078-fc2812ab6181")]
    pub stop: Call,
    /// The run's lifecycle status key — a single key an observer watches to
    /// learn when the run reaches a terminal state and how it ended.
    #[arora(id = "f1a077ed-4007-4f18-b3be-cc3711058515")]
    pub status: Key,
    /// Keys carrying the run's progress feedback, updated as it runs.
    #[arora(id = "60f2f967-16a3-4631-b27c-92445b583d9c")]
    pub feedback: Vec<Key>,
    /// Keys carrying the run's result, written when it terminates.
    #[arora(id = "d6afb314-2bfd-444a-b041-39d9223ba700")]
    pub result: Vec<Key>,
    /// Keys an observer may write to steer a live run (e.g. a moving target).
    #[arora(id = "53d75772-12bd-4a55-a0a5-696b2bc4f6a9")]
    pub update: Vec<Key>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::module::low::TypeRef;
    use arora_types::ty::low::TypeKind;
    use arora_types::AroraType;

    #[test]
    fn task_handle_arora_type_shapes_its_fields() {
        let (ty, registry) = TaskHandle::arora_type_with_registry();
        let TypeKind::Structure(structure) = &ty.kind else {
            panic!("TaskHandle is a structure type");
        };
        let field = |id: &str| {
            structure.fields[&arora_types::Uuid::parse_str(id).unwrap()]
                .type_ref
                .clone()
        };
        // id: a scalar uuid (TaskId is transparent over uuid).
        assert!(matches!(
            field("35a14602-9fb2-4387-b202-21f0fc8cfb0c"),
            TypeRef::Scalar { id } if id == *arora_types::ty::UUID_ID
        ));
        // stop: the nested Call structure, named by Call's id.
        assert!(matches!(
            field("41e5efbb-959a-4aa0-b078-fc2812ab6181"),
            TypeRef::Scalar { id } if id == Call::arora_type_id()
        ));
        // status: a scalar string (Key is transparent over string).
        assert!(matches!(
            field("f1a077ed-4007-4f18-b3be-cc3711058515"),
            TypeRef::Scalar { id } if id == *arora_types::ty::STRING_ID
        ));
        // feedback / result / update: arrays of string keys.
        for id in [
            "60f2f967-16a3-4631-b27c-92445b583d9c",
            "d6afb314-2bfd-444a-b041-39d9223ba700",
            "53d75772-12bd-4a55-a0a5-696b2bc4f6a9",
        ] {
            assert!(matches!(
                field(id),
                TypeRef::Array { id } if id == *arora_types::ty::STRING_ID
            ));
        }
        // The one nested user type — Call — is registered; TaskId and Key are
        // primitives and so are not.
        assert!(registry.contains_key(&Call::arora_type_id()));
    }
}
