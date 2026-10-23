//! URDF inverse kinematics helpers gated behind the `urdf_ik` feature.

use arora_shape::value as vocab;
use arora_shape::Value;

use super::numeric;
use hashbrown::HashMap;
use k::InverseKinematicsSolver;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Cached state for URDF chains shared by IK and FK nodes.
pub struct UrdfKinematicsState {
    pub hash: u64,
    pub dofs: usize,
    pub joint_names: Vec<String>,
    pub chain: k::SerialChain<f32>,
    /// An IK node's last solve: what it solved for and the joint angles it
    /// found. A request equal to it is answered from it, not solved again:
    /// solving from its own solution can land a redundant chain on another
    /// one, so the joints would move with nothing they depend on changed.
    pub(crate) last_solve: Option<(IkRequest, Vec<f32>)>,
}

/// What an IK node solves for: its target, the seed and weights it was
/// given and the solver settings — everything a solve reads but the chain.
/// Without a seed, a solve starts from the joints the chain holds: the
/// node's last solution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IkRequest {
    pub target: IkTarget,
    pub seed: Option<Vec<f32>>,
    pub weights: Option<Vec<f32>>,
    pub max_iters: u32,
    pub tol_pos: f32,
}

/// Where an IK node puts the chain's tip.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum IkTarget {
    /// A position, the tip's orientation free.
    Position([f32; 3]),
    /// A position and an orientation (`[x, y, z, w]`), the orientation
    /// within `tol_rot` radians.
    Pose {
        position: [f32; 3],
        rotation: [f32; 4],
        tol_rot: f32,
    },
}

impl fmt::Debug for UrdfKinematicsState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UrdfKinematicsState")
            .field("hash", &self.hash)
            .field("dofs", &self.dofs)
            .field("joint_names", &self.joint_names)
            .finish()
    }
}

impl UrdfKinematicsState {
    /// Construct a new kinematics state from a serial chain and its joint names.
    pub fn new(hash: u64, chain: k::SerialChain<f32>, joint_names: Vec<String>) -> Self {
        let dofs = chain.dof();
        if dofs > 0 {
            let zero_seed = vec![0.0f32; dofs];
            chain.set_joint_positions_clamped(&zero_seed);
        }
        UrdfKinematicsState {
            hash,
            dofs,
            joint_names,
            chain,
            last_solve: None,
        }
    }

    /// The joint angles that reach `request`'s target: the last solve's when
    /// it asked the same, the chain set back to them, else a new solve's,
    /// kept as the last solve when it succeeds.
    pub(crate) fn solve(&mut self, request: IkRequest) -> Result<Vec<f32>, String> {
        if let Some((last, solution)) = &self.last_solve {
            if *last == request {
                self.chain.set_joint_positions_clamped(solution);
                return Ok(solution.clone());
            }
        }
        let solution = solve_request(self, &request)?;
        self.last_solve = Some((request, solution.clone()));
        Ok(solution)
    }

    /// Produce a record value mapping joint names to solved angles.
    pub fn solution_record(&self, joints: &[f32]) -> Value {
        vocab::record(
            self.joint_names
                .iter()
                .zip(joints.iter())
                .map(|(name, angle)| (name.as_str(), vocab::float(*angle))),
        )
    }
}

/// Key that uniquely identifies a URDF chain configuration for caching.
pub struct IkKey<'a> {
    pub hash: u64,
    pub urdf_xml: &'a str,
    pub root_link: &'a str,
    pub tip_link: &'a str,
}

/// Compute a stable hash for a URDF IK configuration.
pub fn hash_urdf_config(urdf_xml: &str, root_link: &str, tip_link: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    urdf_xml.hash(&mut hasher);
    root_link.hash(&mut hasher);
    tip_link.hash(&mut hasher);
    hasher.finish()
}

/// Build a serial chain between `root` and `tip` from the provided URDF XML.
pub fn build_chain_from_urdf(
    urdf_xml: &str,
    root: &str,
    tip: &str,
) -> Result<(k::SerialChain<f32>, Vec<String>), String> {
    if urdf_xml.trim().is_empty() {
        return Err("URDF XML is empty".to_string());
    }

    let robot = urdf_rs::read_from_string(urdf_xml)
        .map_err(|err| format!("failed to parse URDF: {err}"))?;

    let link_to_joint = k::urdf::link_to_joint_map(&robot);
    let tip_joint = link_to_joint
        .get(tip)
        .ok_or_else(|| format!("tip link '{tip}' not found in URDF"))?;
    let root_joint = link_to_joint
        .get(root)
        .ok_or_else(|| format!("root link '{root}' not found in URDF"))?;

    let chain = k::Chain::<f32>::from(robot);

    let tip_node = chain
        .find(tip_joint)
        .ok_or_else(|| format!("tip joint '{tip_joint}' not found in chain"))?;

    let root_node = if root_joint == k::urdf::ROOT_JOINT_NAME {
        chain
            .iter()
            .next()
            .ok_or_else(|| "URDF chain is empty".to_string())?
    } else {
        chain
            .find(root_joint)
            .ok_or_else(|| format!("root joint '{root_joint}' not found in chain"))?
    };

    let serial = k::SerialChain::from_end_to_root(tip_node, root_node);

    let mut iter = serial.iter();
    let first = iter
        .next()
        .ok_or_else(|| format!("no chain found between '{root}' and '{tip}'"))?;
    let mut last = first;
    for node in iter {
        last = node;
    }

    let first_joint = first.joint().name.clone();
    if root_joint != k::urdf::ROOT_JOINT_NAME && first_joint != *root_joint {
        return Err(format!(
            "root link '{root}' (joint '{root_joint}') is not an ancestor of tip '{tip}'"
        ));
    }

    let tip_joint_name = last.joint().name.clone();
    if tip_joint_name != *tip_joint {
        return Err(format!(
            "tip link '{tip}' (joint '{tip_joint}') is not reachable from root '{root}'"
        ));
    }

    let joint_names: Vec<String> = serial
        .iter_joints()
        .map(|joint| joint.name.clone())
        .collect();

    let dofs = serial.dof();
    if dofs == 0 {
        return Err("selected chain has no movable joints".to_string());
    }

    Ok((serial, joint_names))
}

/// Apply optional joint-space weights to the solver.
fn apply_weights(
    solver: &mut k::JacobianIkSolver<f32>,
    reference: &[f32],
    weights: Option<&[f32]>,
) -> Result<(), String> {
    if let Some(w) = weights {
        solver.set_nullspace_function(Box::new(k::create_reference_positions_nullspace_function(
            reference.to_vec(),
            w.to_vec(),
        )));
    } else {
        solver.clear_nullspace_function();
    }
    Ok(())
}

/// Solve `request` on `state`'s chain, from its seed or, without one, from
/// the joints the chain holds.
fn solve_request(state: &mut UrdfKinematicsState, request: &IkRequest) -> Result<Vec<f32>, String> {
    let seed = match &request.seed {
        Some(seed) => seed.clone(),
        None => {
            let joints = state.chain.joint_positions();
            if joints.len() == state.dofs {
                joints
            } else {
                vec![0.0; state.dofs]
            }
        }
    };
    let mut solver = k::JacobianIkSolver::default();
    let weights = request.weights.as_deref();
    match request.target {
        IkTarget::Position(position) => solve_position(
            state,
            &mut solver,
            position,
            &seed,
            weights,
            request.max_iters,
            request.tol_pos,
        ),
        IkTarget::Pose {
            position,
            rotation,
            tol_rot,
        } => solve_pose(
            state,
            &mut solver,
            position,
            rotation,
            &seed,
            weights,
            request.max_iters,
            request.tol_pos,
            tol_rot,
        ),
    }
}

/// Solve for joint positions that reach `target_pos` while respecting `weights`.
pub fn solve_position(
    state: &mut UrdfKinematicsState,
    solver: &mut k::JacobianIkSolver<f32>,
    target_pos: [f32; 3],
    seed: &[f32],
    weights: Option<&[f32]>,
    max_iters: u32,
    tol_pos: f32,
) -> Result<Vec<f32>, String> {
    state
        .chain
        .set_joint_positions(seed)
        .map_err(|err| format!("failed to apply joint seed: {err}"))?;

    apply_weights(solver, seed, weights)?;

    solver.num_max_try = max_iters.max(1) as usize;
    solver.allowable_target_distance = tol_pos;
    solver.allowable_target_angle = std::f32::consts::PI;

    let target_pose = k::Isometry3::from_parts(
        k::Translation3::new(target_pos[0], target_pos[1], target_pos[2]),
        k::UnitQuaternion::identity(),
    );

    let constraints = k::Constraints {
        rotation_x: false,
        rotation_y: false,
        rotation_z: false,
        ..Default::default()
    };

    solver
        .solve_with_constraints(&state.chain, &target_pose, &constraints)
        .map_err(|err| format!("IK solve failed: {err}"))?;

    Ok(state.chain.joint_positions())
}

/// Read a joint's value as one number: a number, a boolean as `1` or `0`,
/// the first item of a numeric vector, the one item of a sequence, an
/// enumeration's payload, or the first field of a structure of numbers.
fn scalar_from_value(value: &Value) -> Result<f32, String> {
    let mismatch = || format!("expected numeric scalar, received {}", describe(value));
    match value {
        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::ArrayF32(xs) => xs
            .first()
            .copied()
            .ok_or_else(|| "vector is empty".to_string()),
        Value::ArrayValue(items) if items.len() == 1 => scalar_from_value(&items[0]),
        Value::Enumeration(e) => scalar_from_value(&e.value),
        Value::Structure(s)
            if !s.fields.is_empty() && s.fields.iter().all(|f| is_number(&f.value)) =>
        {
            Ok(numeric::as_float(value))
        }
        _ if is_number(value) => Ok(numeric::as_float(value)),
        _ => Err(mismatch()),
    }
}

fn is_number(value: &Value) -> bool {
    matches!(
        value,
        Value::F32(_)
            | Value::F64(_)
            | Value::U8(_)
            | Value::U16(_)
            | Value::U32(_)
            | Value::U64(_)
            | Value::I8(_)
            | Value::I16(_)
            | Value::I32(_)
            | Value::I64(_)
    )
}

/// What `value` is, for an error message: its geometry type, or its form.
pub(crate) fn describe(value: &Value) -> &'static str {
    if let Some(kind) = arora_geometry::kind(value) {
        return kind.name();
    }
    match value {
        Value::Unit => "unit",
        Value::Boolean(_) => "boolean",
        Value::String(_) => "text",
        Value::ArrayF32(_) => "vector",
        Value::ArrayValue(_) => "sequence",
        Value::KeyValue(_) => "record",
        Value::Structure(_) => "structure",
        Value::Enumeration(_) => "enumeration",
        v if is_number(v) => "number",
        _ => "value",
    }
}

fn align_sequence_with_defaults(
    values: &[f32],
    expected: usize,
    joint_names: &[String],
    defaults: &HashMap<&str, f32>,
) -> Vec<f32> {
    let mut result = Vec::with_capacity(expected);
    for (index, joint) in joint_names.iter().enumerate() {
        if let Some(value) = values.get(index) {
            result.push(*value);
        } else if let Some(default_value) = defaults.get(joint.as_str()) {
            result.push(*default_value);
        } else {
            result.push(0.0);
        }
    }
    result
}

/// Coerce input joint data into a vector matching the chain joint order.
pub fn fetch_joint_vector(
    value: &Value,
    expected: usize,
    defaults: Option<&[(String, f32)]>,
    joint_names: &[String],
) -> Result<Vec<f32>, String> {
    let mut default_map: HashMap<&str, f32> = HashMap::new();
    if let Some(entries) = defaults {
        for (name, angle) in entries {
            default_map.insert(name.as_str(), *angle);
        }
    }

    match value {
        Value::KeyValue(record) => {
            let mut result = Vec::with_capacity(expected);
            for joint in joint_names {
                let entry = record
                    .fields
                    .get(joint.as_str())
                    .and_then(|field| field.value.as_deref());
                if let Some(entry) = entry {
                    result.push(scalar_from_value(entry)?);
                } else if let Some(default_value) = default_map.get(joint.as_str()) {
                    result.push(*default_value);
                } else {
                    result.push(0.0);
                }
            }
            Ok(result)
        }
        Value::ArrayValue(items) => {
            let mut sequence = Vec::with_capacity(items.len());
            for item in items {
                sequence.push(scalar_from_value(item)?);
            }
            Ok(align_sequence_with_defaults(
                &sequence,
                expected,
                joint_names,
                &default_map,
            ))
        }
        _ => {
            let sequence = numeric::as_vector(value);
            Ok(align_sequence_with_defaults(
                &sequence,
                expected,
                joint_names,
                &default_map,
            ))
        }
    }
}

/// Apply joint values to the cached serial chain.
pub fn apply_joint_positions(
    state: &mut UrdfKinematicsState,
    joints: &[f32],
) -> Result<(), String> {
    if joints.len() != state.dofs {
        return Err(format!(
            "joint vector length {} does not match chain DoF {}",
            joints.len(),
            state.dofs
        ));
    }
    state
        .chain
        .set_joint_positions(joints)
        .map_err(|err| format!("failed to apply joint values: {err}"))?;
    Ok(())
}

/// Extract the current tip pose (position + quaternion) from the chain.
pub fn tip_pose(state: &UrdfKinematicsState) -> ([f32; 3], [f32; 4]) {
    let end = state.chain.end_transform();
    let pos = end.translation.vector;
    let rot = end.rotation;
    ([pos.x, pos.y, pos.z], [rot.i, rot.j, rot.k, rot.w])
}

#[allow(clippy::too_many_arguments)]
/// Solve for joint positions that reach both `target_pos` and `target_rot`.
pub fn solve_pose(
    state: &mut UrdfKinematicsState,
    solver: &mut k::JacobianIkSolver<f32>,
    target_pos: [f32; 3],
    target_rot: [f32; 4],
    seed: &[f32],
    weights: Option<&[f32]>,
    max_iters: u32,
    tol_pos: f32,
    tol_rot: f32,
) -> Result<Vec<f32>, String> {
    state
        .chain
        .set_joint_positions(seed)
        .map_err(|err| format!("failed to apply joint seed: {err}"))?;

    apply_weights(solver, seed, weights)?;

    solver.num_max_try = max_iters.max(1) as usize;
    solver.allowable_target_distance = tol_pos;
    solver.allowable_target_angle = tol_rot;

    let rotation = k::UnitQuaternion::new_normalize(k::nalgebra::Quaternion::new(
        target_rot[3],
        target_rot[0],
        target_rot[1],
        target_rot[2],
    ));
    let target_pose = k::Isometry3::from_parts(
        k::Translation3::new(target_pos[0], target_pos[1], target_pos[2]),
        rotation,
    );

    solver
        .solve(&state.chain, &target_pose)
        .map_err(|err| format!("IK solve failed: {err}"))?;

    Ok(state.chain.joint_positions())
}

/// Extract the numeric components from a supported value type.
pub fn vector_from_value(value: &Value, label: &str) -> Result<Vec<f32>, String> {
    if let Value::ArrayF32(xs) = value {
        return Ok(xs.clone());
    }
    arora_geometry::as_vec2(value)
        .map(|a| a.to_vec())
        .or_else(|| arora_geometry::as_vec3(value).map(|a| a.to_vec()))
        .or_else(|| arora_geometry::as_vec4(value).map(|a| a.to_vec()))
        .or_else(|| arora_geometry::as_quat(value).map(|a| a.to_vec()))
        .ok_or_else(|| {
            format!(
                "{label} expects a numeric vector, received {}",
                describe(value)
            )
        })
}

/// Interpet a [`Value`] as a quaternion `[x, y, z, w]`.
pub fn quat_from_value(value: &Value, label: &str) -> Result<[f32; 4], String> {
    arora_geometry::as_quat(value)
        .or_else(|| arora_geometry::as_vec4(value))
        .or_else(|| match value {
            Value::ArrayF32(xs) if xs.len() == 4 => Some([xs[0], xs[1], xs[2], xs[3]]),
            _ => None,
        })
        .ok_or_else(|| {
            format!(
                "{label} expects a quaternion (x, y, z, w), received {}",
                describe(value)
            )
        })
}

/// The URDF chains a host has built, by the hash of their configuration
/// ([`hash_urdf_config`]), with their joint names: a cache, so a node whose
/// state lives outside the runtime does not parse its URDF at every call.
pub(crate) type Chains = std::collections::HashMap<u64, (k::SerialChain<f32>, Vec<String>)>;

/// The hash of the chain configuration `params` names, as
/// [`kinematics_state_mut`](crate::eval::GraphRuntime::kinematics_state_mut)
/// keys it; `None` without URDF XML.
fn configuration_hash(params: &crate::params::Params) -> Option<u64> {
    let urdf_xml = params
        .urdf_xml
        .as_deref()
        .filter(|xml| !xml.trim().is_empty())?;
    Some(hash_urdf_config(
        urdf_xml,
        params.root_link.as_deref().unwrap_or("base_link"),
        params.tip_link.as_deref().unwrap_or("tool0"),
    ))
}

/// The kinematics state a URDF node starts a call with: the chain `chains`
/// holds for its configuration, its joints and its last solve set from
/// `state` (what [`encode_state`] made of the state its last call left), and
/// `None` when `chains` has not built the chain yet.
pub(crate) fn prepared_state(
    chains: &Chains,
    params: &crate::params::Params,
    state: Option<&Value>,
) -> Option<UrdfKinematicsState> {
    let hash = configuration_hash(params)?;
    let (chain, joint_names) = chains.get(&hash)?;
    let mut prepared = UrdfKinematicsState::new(hash, chain.clone(), joint_names.clone());
    let Some(Value::ArrayValue(parts)) = state else {
        return Some(prepared);
    };
    if let Some(Value::ArrayF32(joints)) = parts.first() {
        if joints.len() == prepared.dofs {
            prepared.chain.set_joint_positions_clamped(joints);
        }
    }
    prepared.last_solve = parts.get(1).and_then(decode_solve);
    Some(prepared)
}

/// Keep the chain `state` holds in `chains`, for the next call of a node of
/// the same configuration.
pub(crate) fn keep_chain(chains: &mut Chains, state: &UrdfKinematicsState) {
    chains
        .entry(state.hash)
        .or_insert_with(|| (state.chain.clone(), state.joint_names.clone()));
}

/// A URDF node's state as a value: its joints, and its last solve.
pub(crate) fn encode_state(state: &UrdfKinematicsState) -> Value {
    Value::ArrayValue(vec![
        Value::ArrayF32(state.chain.joint_positions()),
        state
            .last_solve
            .as_ref()
            .map_or(Value::Unit, |(request, solution)| {
                encode_solve(request, solution)
            }),
    ])
}

fn optional_floats(values: &Option<Vec<f32>>) -> Value {
    Value::Option(
        values
            .clone()
            .map(|values| Box::new(Value::ArrayF32(values))),
    )
}

fn encode_solve(request: &IkRequest, solution: &[f32]) -> Value {
    let target = match &request.target {
        IkTarget::Position(position) => Value::ArrayF32(position.to_vec()),
        IkTarget::Pose {
            position,
            rotation,
            tol_rot,
        } => Value::ArrayF32(
            position
                .iter()
                .chain(rotation)
                .copied()
                .chain([*tol_rot])
                .collect(),
        ),
    };
    Value::ArrayValue(vec![
        target,
        optional_floats(&request.seed),
        optional_floats(&request.weights),
        Value::U32(request.max_iters),
        Value::F32(request.tol_pos),
        Value::ArrayF32(solution.to_vec()),
    ])
}

fn decode_solve(value: &Value) -> Option<(IkRequest, Vec<f32>)> {
    let Value::ArrayValue(parts) = value else {
        return None;
    };
    let floats = |value: &Value| match value {
        Value::ArrayF32(values) => Some(values.clone()),
        _ => None,
    };
    let optional = |value: &Value| match value {
        Value::Option(Some(values)) => floats(values),
        _ => None,
    };
    let target = floats(parts.first()?)?;
    let target = match target.as_slice() {
        [x, y, z] => IkTarget::Position([*x, *y, *z]),
        [x, y, z, qx, qy, qz, qw, tol_rot] => IkTarget::Pose {
            position: [*x, *y, *z],
            rotation: [*qx, *qy, *qz, *qw],
            tol_rot: *tol_rot,
        },
        _ => return None,
    };
    let (Value::U32(max_iters), Value::F32(tol_pos)) = (parts.get(3)?, parts.get(4)?) else {
        return None;
    };
    Some((
        IkRequest {
            target,
            seed: optional(parts.get(1)?),
            weights: optional(parts.get(2)?),
            max_iters: *max_iters,
            tol_pos: *tol_pos,
        },
        floats(parts.get(5)?)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_shape::value::{Structure, StructureField};

    #[test]
    fn a_joint_value_reads_as_one_number() {
        let structure = |fields: Vec<Value>| {
            Value::Structure(Structure {
                id: uuid::Uuid::from_u128(0xF0),
                fields: fields
                    .into_iter()
                    .enumerate()
                    .map(|(i, value)| StructureField {
                        id: uuid::Uuid::from_u128(i as u128),
                        value: Box::new(value),
                    })
                    .collect(),
            })
        };
        assert_eq!(scalar_from_value(&Value::F64(0.5)), Ok(0.5));
        assert_eq!(scalar_from_value(&Value::I32(-2)), Ok(-2.0));
        assert_eq!(scalar_from_value(&Value::Boolean(true)), Ok(1.0));
        assert_eq!(scalar_from_value(&Value::ArrayF32(vec![3.0, 4.0])), Ok(3.0));
        assert_eq!(
            scalar_from_value(&Value::ArrayValue(vec![Value::F32(5.0)])),
            Ok(5.0)
        );
        assert_eq!(
            scalar_from_value(&vocab::enumeration("on", vocab::float(6.0))),
            Ok(6.0)
        );
        assert_eq!(
            scalar_from_value(&arora_geometry::vec3([7.0, 8.0, 9.0])),
            Ok(7.0)
        );
        assert_eq!(
            scalar_from_value(&structure(vec![Value::F64(1.5), Value::F32(2.0)])),
            Ok(1.5)
        );

        assert_eq!(
            scalar_from_value(&vocab::text("x")),
            Err("expected numeric scalar, received text".to_string())
        );
        assert_eq!(
            scalar_from_value(&arora_geometry::transform(
                arora_geometry::Transform::IDENTITY
            )),
            Err("expected numeric scalar, received transform".to_string())
        );
        assert_eq!(
            scalar_from_value(&structure(vec![Value::F32(1.0), vocab::text("x")])),
            Err("expected numeric scalar, received structure".to_string())
        );
        assert_eq!(
            scalar_from_value(&Value::ArrayF32(vec![])),
            Err("vector is empty".to_string())
        );
    }
}
