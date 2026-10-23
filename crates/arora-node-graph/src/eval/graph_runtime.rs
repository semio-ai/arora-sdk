//! What a node's evaluation reads and writes besides its ports: the clock,
//! the node's state, and the runs it requests.

use crate::types::NodeId;
use arora_shape::Value;
use hashbrown::{hash_map::Entry, HashMap};
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

/// State of a [`TaskRun`](crate::types::NodeType::TaskRun) node's hosted run.
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

/// State of a [`Spawn`](crate::types::NodeType::Spawn) node: the arguments
/// and the `when` value it last saw, which the next evaluation compares
/// against to decide whether to spawn.
#[derive(Debug, Default)]
pub struct SpawnState {
    pub args: Vec<(Uuid, Value)>,
    pub when: Option<Value>,
}

/// A task run a [`Spawn`](crate::types::NodeType::Spawn) node requested
/// during an evaluation, for the host to start once the evaluation is done:
/// `function` addressed to `module`, with its call args by parameter id.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnRequest {
    pub module: Option<Uuid>,
    pub function: Uuid,
    pub args: Vec<(Uuid, Value)>,
}

/// What a node's evaluation reads and writes besides its ports.
#[derive(Debug, Default)]
pub struct GraphRuntime {
    /// Current graph time in seconds.
    pub t: f32,
    /// Step delta in seconds for the active evaluation.
    pub dt: f32,
    /// Task runs requested by spawn nodes during the current evaluation, in
    /// evaluation order — the host starts them after the evaluation.
    pub spawns: Vec<SpawnRequest>,
    /// Per-node persistent state for smoothing, kinematics, and other stateful nodes.
    pub node_states: HashMap<NodeId, NodeRuntimeState>,
}

impl GraphRuntime {
    /// Fetch the spring state for `node_id`, creating or reinitialising it from `flat` as needed.
    pub fn spring_state_mut<'a>(
        &'a mut self,
        node_id: &str,
        flat: &FlatValue,
    ) -> &'a mut SpringState {
        match self.node_states.entry(node_id.to_string()) {
            Entry::Occupied(mut occupied) => {
                {
                    let state = occupied.get_mut();
                    match state {
                        NodeRuntimeState::Spring(inner) => {
                            if inner.layout != flat.layout {
                                inner.reset(flat);
                            }
                        }
                        _ => {
                            *state = NodeRuntimeState::Spring(SpringState::new(flat));
                        }
                    }
                }
                match occupied.into_mut() {
                    NodeRuntimeState::Spring(inner) => inner,
                    _ => unreachable!(),
                }
            }
            Entry::Vacant(vacant) => {
                match vacant.insert(NodeRuntimeState::Spring(SpringState::new(flat))) {
                    NodeRuntimeState::Spring(inner) => inner,
                    _ => unreachable!(),
                }
            }
        }
    }

    /// Fetch the damp state for `node_id`, creating or reinitialising it from `flat` as needed.
    pub fn damp_state_mut<'a>(&'a mut self, node_id: &str, flat: &FlatValue) -> &'a mut DampState {
        match self.node_states.entry(node_id.to_string()) {
            Entry::Occupied(mut occupied) => {
                {
                    let state = occupied.get_mut();
                    match state {
                        NodeRuntimeState::Damp(inner) => {
                            if inner.layout != flat.layout {
                                inner.reset(flat);
                            }
                        }
                        _ => {
                            *state = NodeRuntimeState::Damp(DampState::new(flat));
                        }
                    }
                }
                match occupied.into_mut() {
                    NodeRuntimeState::Damp(inner) => inner,
                    _ => unreachable!(),
                }
            }
            Entry::Vacant(vacant) => {
                match vacant.insert(NodeRuntimeState::Damp(DampState::new(flat))) {
                    NodeRuntimeState::Damp(inner) => inner,
                    _ => unreachable!(),
                }
            }
        }
    }

    /// Fetch the slew state for `node_id`, creating or reinitialising it from `flat` as needed.
    pub fn slew_state_mut<'a>(&'a mut self, node_id: &str, flat: &FlatValue) -> &'a mut SlewState {
        match self.node_states.entry(node_id.to_string()) {
            Entry::Occupied(mut occupied) => {
                {
                    let state = occupied.get_mut();
                    match state {
                        NodeRuntimeState::Slew(inner) => {
                            if inner.layout != flat.layout {
                                inner.reset(flat);
                            }
                        }
                        _ => {
                            *state = NodeRuntimeState::Slew(SlewState::new(flat));
                        }
                    }
                }
                match occupied.into_mut() {
                    NodeRuntimeState::Slew(inner) => inner,
                    _ => unreachable!(),
                }
            }
            Entry::Vacant(vacant) => {
                match vacant.insert(NodeRuntimeState::Slew(SlewState::new(flat))) {
                    NodeRuntimeState::Slew(inner) => inner,
                    _ => unreachable!(),
                }
            }
        }
    }

    #[cfg(feature = "urdf_ik")]
    /// Fetch the cached URDF chain for `node_id`, rebuilding it if the configuration hash
    /// changes.
    pub fn kinematics_state_mut<'a>(
        &'a mut self,
        node_id: &str,
        key: IkKey<'_>,
    ) -> Result<&'a mut UrdfKinematicsState, String> {
        let build_state = || -> Result<UrdfKinematicsState, String> {
            let (chain, joint_names) =
                build_chain_from_urdf(key.urdf_xml, key.root_link, key.tip_link)?;
            Ok(UrdfKinematicsState::new(key.hash, chain, joint_names))
        };

        match self.node_states.entry(node_id.to_string()) {
            Entry::Occupied(occupied) => {
                let state = occupied.into_mut();
                match state {
                    NodeRuntimeState::UrdfKinematics(inner) => {
                        if inner.hash != key.hash {
                            *inner = build_state()?;
                        }
                        Ok(inner)
                    }
                    _ => {
                        *state = NodeRuntimeState::UrdfKinematics(build_state()?);
                        match state {
                            NodeRuntimeState::UrdfKinematics(inner) => Ok(inner),
                            _ => unreachable!(),
                        }
                    }
                }
            }
            Entry::Vacant(vacant) => {
                match vacant.insert(NodeRuntimeState::UrdfKinematics(build_state()?)) {
                    NodeRuntimeState::UrdfKinematics(inner) => Ok(inner),
                    _ => unreachable!(),
                }
            }
        }
    }
}
