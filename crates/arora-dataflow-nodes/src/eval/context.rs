//! What a node's evaluation reads and writes besides its ports: the clock,
//! the node's state, and the runs it requests.

use arora_types::value::Value;
use uuid::Uuid;

#[cfg(feature = "urdf_ik")]
use super::urdfik::{build_chain_from_urdf, IkKey, UrdfKinematicsState};
use super::value_layout::{FlatValue, ValueLayout};

/// Internal integration state for a spring node. Values remain flattened for efficiency.
#[derive(Clone, Debug)]
pub struct SpringState {
    /// Layout used to reconstruct structured values from the flattened buffers.
    pub layout: ValueLayout,
    /// Current integrated position.
    pub position: Vec<f32>,
    /// Current integrated velocity.
    pub velocity: Vec<f32>,
    /// Most recent requested target position.
    pub target: Vec<f32>,
}

impl SpringState {
    /// Create a new state seeded with the provided flat value.
    fn new(flat: &FlatValue) -> Self {
        let len = flat.data.len();
        SpringState {
            layout: flat.layout.clone(),
            position: flat.data.clone(),
            velocity: vec![0.0; len],
            target: flat.data.clone(),
        }
    }

    /// Reset the layout and buffers to match the most recent input.
    fn reset(&mut self, flat: &FlatValue) {
        let len = flat.data.len();
        self.layout = flat.layout.clone();
        self.position = flat.data.clone();
        self.velocity = vec![0.0; len];
        self.target = flat.data.clone();
    }
}

/// Integration state for a damp node.
#[derive(Clone, Debug)]
pub struct DampState {
    /// Layout used to reconstruct structured values from the flattened buffers.
    pub layout: ValueLayout,
    /// Current smoothed value.
    pub value: Vec<f32>,
}

impl DampState {
    /// Create a new state seeded with the provided flat value.
    fn new(flat: &FlatValue) -> Self {
        DampState {
            layout: flat.layout.clone(),
            value: flat.data.clone(),
        }
    }

    /// Reset the cached layout and value to match the incoming data.
    fn reset(&mut self, flat: &FlatValue) {
        self.layout = flat.layout.clone();
        self.value = flat.data.clone();
    }
}

/// Integration state for a slew node.
#[derive(Clone, Debug)]
pub struct SlewState {
    /// Layout used to reconstruct structured values from the flattened buffers.
    pub layout: ValueLayout,
    /// Current rate-limited value.
    pub value: Vec<f32>,
}

impl SlewState {
    /// Create a new state seeded with the provided flat value.
    fn new(flat: &FlatValue) -> Self {
        SlewState {
            layout: flat.layout.clone(),
            value: flat.data.clone(),
        }
    }

    /// Reset the cached layout and value to match the incoming data.
    fn reset(&mut self, flat: &FlatValue) {
        self.layout = flat.layout.clone();
        self.value = flat.data.clone();
    }
}

/// State stored for each node that requires persistence across frames.
#[derive(Debug)]
pub enum NodeRuntimeState {
    Spring(SpringState),
    Damp(DampState),
    Slew(SlewState),
    #[cfg(feature = "urdf_ik")]
    UrdfKinematics(UrdfKinematicsState),
    TaskRun(TaskRunState),
    Spawn(SpawnState),
}

/// State of a [`TaskRun`](crate::kind::Kind::TaskRun) node's hosted run.
#[derive(Debug, Default)]
pub struct TaskRunState {
    /// The terminal status value the run latched on, once it has ended. A
    /// latched run's function is not invoked again; the node keeps emitting
    /// this value until its fragment is pruned.
    pub latched: Option<Value>,
    /// The out-parameter record of the last invocation — what the node keeps
    /// emitting on `mutated` once latched (the function's final word, e.g. a
    /// rest token).
    pub outputs: Option<Value>,
}

/// State of a [`Spawn`](crate::kind::Kind::Spawn) node: the arguments
/// and the `when` value it last saw, which the next evaluation compares
/// against to decide whether to spawn.
#[derive(Debug, Default)]
pub struct SpawnState {
    pub args: Vec<(Uuid, Value)>,
    pub when: Option<Value>,
}

/// A task run a [`Spawn`](crate::kind::Kind::Spawn) node requested
/// during an evaluation, for the host to start once the evaluation is done:
/// `function` addressed to `module`, with its call args by parameter id.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnRequest {
    pub module: Option<Uuid>,
    pub function: Uuid,
    pub args: Vec<(Uuid, Value)>,
}

/// What a node's evaluation reads and writes besides its ports: one call's
/// clock, the node's state, and the runs it requests.
#[derive(Debug, Default)]
pub struct Context {
    /// Current graph time in seconds.
    pub t: f32,
    /// Step delta in seconds for the active evaluation.
    pub dt: f32,
    /// Task runs requested by spawn nodes during the current evaluation, in
    /// evaluation order — the host starts them after the evaluation.
    pub spawns: Vec<SpawnRequest>,
    /// The node's state, for a stateful node: what its last call left, and
    /// what this call leaves for the next.
    pub state: Option<NodeRuntimeState>,
}

impl Context {
    /// The spring state, created or reinitialised from `flat` when the node
    /// holds none of its layout.
    pub fn spring_state_mut(&mut self, flat: &FlatValue) -> &mut SpringState {
        if !matches!(self.state, Some(NodeRuntimeState::Spring(_))) {
            self.state = Some(NodeRuntimeState::Spring(SpringState::new(flat)));
        }
        let Some(NodeRuntimeState::Spring(inner)) = &mut self.state else {
            unreachable!("the spring state is set above");
        };
        if inner.layout != flat.layout {
            inner.reset(flat);
        }
        inner
    }

    /// The damp state, created or reinitialised from `flat` when the node
    /// holds none of its layout.
    pub fn damp_state_mut(&mut self, flat: &FlatValue) -> &mut DampState {
        if !matches!(self.state, Some(NodeRuntimeState::Damp(_))) {
            self.state = Some(NodeRuntimeState::Damp(DampState::new(flat)));
        }
        let Some(NodeRuntimeState::Damp(inner)) = &mut self.state else {
            unreachable!("the damp state is set above");
        };
        if inner.layout != flat.layout {
            inner.reset(flat);
        }
        inner
    }

    /// The slew state, created or reinitialised from `flat` when the node
    /// holds none of its layout.
    pub fn slew_state_mut(&mut self, flat: &FlatValue) -> &mut SlewState {
        if !matches!(self.state, Some(NodeRuntimeState::Slew(_))) {
            self.state = Some(NodeRuntimeState::Slew(SlewState::new(flat)));
        }
        let Some(NodeRuntimeState::Slew(inner)) = &mut self.state else {
            unreachable!("the slew state is set above");
        };
        if inner.layout != flat.layout {
            inner.reset(flat);
        }
        inner
    }

    /// The node's URDF chain, built again when its configuration's hash
    /// changes.
    #[cfg(feature = "urdf_ik")]
    pub fn kinematics_state_mut(
        &mut self,
        key: IkKey<'_>,
    ) -> Result<&mut UrdfKinematicsState, String> {
        let current = matches!(
            &self.state,
            Some(NodeRuntimeState::UrdfKinematics(inner)) if inner.hash == key.hash
        );
        if !current {
            let (chain, joint_names) =
                build_chain_from_urdf(key.urdf_xml, key.root_link, key.tip_link)?;
            self.state = Some(NodeRuntimeState::UrdfKinematics(UrdfKinematicsState::new(
                key.hash,
                chain,
                joint_names,
            )));
        }
        let Some(NodeRuntimeState::UrdfKinematics(inner)) = &mut self.state else {
            unreachable!("the kinematics state is set above");
        };
        Ok(inner)
    }
}
