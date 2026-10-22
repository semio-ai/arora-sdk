//! Load-time placement of detached computations before their readers.
//!
//! Placement preserves port references and makes each used detached node an
//! attached `_on_change` leaf. A store-bound output is written when that leaf
//! runs: a tree node reading its key sees the new value in the same tick,
//! instead of the next tick. Sinks run before the root rather than after it.
//! Stateful functions, writes during a tick, errors, and store-key feedback
//! therefore need not behave like demand-driven detached evaluation.
use std::collections::{HashMap, HashSet};

use arora_types::value::Value;
use uuid::Uuid;

use crate::error::BehaviorTreeError;
use crate::nodes::{PARALLEL_FUNCTION_ID, WITH_FUNCTION_ID};
use crate::schema::{Expression, Node, NodeParameterId, _ON_CHANGE_PARAM_ID, _RET_PARAM_ID};
use crate::variable::{VariableCell, VariableResolver};
use crate::{load_behavior_tree_nodes_with, FunctionIndex};

/// Where detached groups execute relative to the attached tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementScope {
    /// Immediately before the common ancestor of every reader path. Sinks
    /// execute before the root, including sinks also read inside the tree.
    Readers,
    /// Before the root, even when a reader's branch is not reached.
    Root,
}

fn invalid(message: String) -> BehaviorTreeError {
    BehaviorTreeError::InconsistentTreeError { message }
}

/// A stable, domain-separated id from its anchor and first listed group node.
/// Collision probing also reserves generated variable ids against file ids.
fn fresh(anchor: Uuid, group: Uuid, tag: u128, used: &mut HashSet<Uuid>) -> Uuid {
    let mut value = anchor.as_u128() ^ group.as_u128().rotate_left(64) ^ tag;
    loop {
        let id = Uuid::from_u128(value);
        if used.insert(id) {
            return id;
        }
        value = value.wrapping_add(1);
    }
}
const WITH: u128 = 0x57544800000040008000000000000000;
const GROUP: u128 = 0x47525000000040008000000000000000;
const PORT: u128 = 0x504f5200000040008000000000000000;

// The incoming child index distinguishes repeated children of one parent.
type ReaderPath = Vec<(Uuid, usize)>;

/// Place the used detached nodes in `nodes`, without evaluating any function
/// or writing any cell. The first node is the root. `functions`, `resolver`
/// and `names` are the metadata used when loading the resulting tree: mutable
/// parameters and store bindings decide outputs and sinks, so placement needs
/// the same information as detached lowering. Resolver values do not affect
/// placement; its mapping must be stable across this call and loading.
///
/// Connected components of detached reads execute in data order in separate
/// `PARALLEL` nodes. A `WITH` executes these groups before its body and returns
/// the body's status. Groups at one anchor follow their first listed node.
/// Every path of a shared reader contributes to the common ancestor. A root
/// reader or any sink forces a root anchor; unused non-sinks are omitted.
/// Inert loose controls are retained. Generated ids and list order are stable,
/// including when an existing id collides with a generated one.
///
/// Detached validation is shared with runtime lowering. Attached cycles and
/// missing children are rejected, because they have no finite reader paths.
/// Execute the result with incremental caching enabled.
pub fn place_detached(
    mut nodes: Vec<Node>,
    scope: PlacementScope,
    functions: &FunctionIndex,
    resolver: &VariableResolver,
    names: &HashMap<Uuid, String>,
) -> Result<Vec<Node>, BehaviorTreeError> {
    let tree = load_behavior_tree_nodes_with(nodes.clone(), resolver, names)?;
    tree.lower_detached(functions)?;
    let analysis = tree.detached.placement();
    let detached = tree.detached.nodes();
    let root = tree.root.id;
    let mut paths: HashMap<Uuid, Vec<ReaderPath>> = HashMap::new();
    fn walk(
        id: Uuid,
        tree: &crate::BehaviorTree,
        edge: usize,
        path: &mut ReaderPath,
        paths: &mut HashMap<Uuid, Vec<ReaderPath>>,
    ) -> Result<(), BehaviorTreeError> {
        if path.iter().any(|(node, _)| *node == id) {
            return Err(invalid(format!("attached cycle at node {id}")));
        }
        let node = tree
            .node_index
            .get(&id)
            .ok_or_else(|| invalid(format!("missing attached child {id}")))?;
        path.push((id, edge));
        paths.entry(id).or_default().push(path.clone());
        for (edge, child) in node.children.iter().flatten().enumerate() {
            walk(*child, tree, edge, path, paths)?;
        }
        path.pop();
        Ok(())
    }
    walk(root, &tree, 0, &mut Vec::new(), &mut paths)?;
    let active: HashSet<usize> = (0..detached.len())
        .filter(|at| !analysis.unused.contains(&detached[*at].id))
        .collect();
    let mut edges = analysis.reads.clone();
    for (reader, reads) in analysis.reads.iter().enumerate() {
        for read in reads {
            edges[*read].push(reader);
        }
    }
    let mut used: HashSet<Uuid> = nodes
        .iter()
        .map(|node| node.id)
        .chain(names.keys().copied())
        .chain(tree.variables.borrow().keys().copied())
        .collect();
    // A detached return feeding an input bound to a key uses a local cell,
    // not that key. Preserve the loader's isolation when attachment changes.
    let mut isolated = HashMap::new();
    for node in &mut nodes {
        let mut parameters: Vec<_> = node.arguments.keys().copied().collect();
        parameters.sort_unstable();
        for parameter in parameters {
            let port = NodeParameterId {
                node: node.id,
                parameter,
            };
            let argument = &node.arguments[&parameter];
            let stored = match argument {
                Expression::VariableId(id) => tree.variables.borrow().get(id).cloned(),
                Expression::Variable(cell) => Some(cell.clone()),
                _ => None,
            };
            if !matches!(stored, Some(VariableCell::Stored(_))) || tree.detached.publishes(&port) {
                continue;
            }
            if let Some(VariableCell::Local(cell)) = tree.node_arg_variables.get(&port) {
                let identity = std::rc::Rc::as_ptr(cell) as usize;
                let variable = *isolated
                    .entry(identity)
                    .or_insert_with(|| fresh(node.id, parameter, PORT, &mut used));
                node.arguments
                    .insert(parameter, Expression::VariableId(variable));
            }
        }
    }
    let mut seen = HashSet::new();
    let mut anchored: HashMap<Uuid, Vec<Node>> = HashMap::new();
    for first in 0..detached.len() {
        if seen.contains(&first) {
            continue;
        }
        let mut group = HashSet::new();
        let mut stack = vec![first];
        while let Some(at) = stack.pop() {
            if group.insert(at) {
                stack.extend(edges[at].iter().copied());
            }
        }
        seen.extend(group.iter().copied());
        // Components come from the file's whole dependency graph. An unused
        // downstream node may connect two live computations, but never runs.
        group.retain(|at| active.contains(at));
        if group.is_empty() {
            continue;
        }
        let mut common: Option<ReaderPath> = None;
        for (reader, pulls) in &analysis.pulls {
            if !pulls.iter().any(|at| group.contains(at)) {
                continue;
            }
            for path in &paths[reader] {
                match &mut common {
                    None => common = Some(path.clone()),
                    Some(common) => {
                        let length = common.iter().zip(path).take_while(|(a, b)| a == b).count();
                        common.truncate(length);
                    }
                }
            }
        }
        let anchor = if scope == PlacementScope::Root
            || analysis.sinks.iter().any(|at| group.contains(at))
        {
            root
        } else {
            common
                .as_ref()
                .and_then(|path| path.last())
                .expect("active group has readers or a sink")
                .0
        };
        let children = analysis
            .order
            .iter()
            .filter(|at| group.contains(at))
            .map(|at| detached[*at].id)
            .collect();
        anchored.entry(anchor).or_default().push(Node {
            id: fresh(anchor, detached[first].id, GROUP, &mut used),
            function: PARALLEL_FUNCTION_ID,
            children: Some(children),
            ..Node::default()
        });
    }
    let detached_ids: HashSet<Uuid> = detached.iter().map(|node| node.id).collect();
    nodes.retain(|node| !analysis.unused.contains(&node.id));
    for node in &mut nodes {
        if detached_ids.contains(&node.id) {
            node.children = None;
            node.arguments
                .insert(_ON_CHANGE_PARAM_ID, Expression::Value(Value::Boolean(true)));
            // An attached return needs an explicit binding; detached loading
            // supplies an implicit port even when the file has none.
            node.arguments.entry(_RET_PARAM_ID).or_insert_with(|| {
                Expression::VariableId(fresh(node.id, Uuid::nil(), PORT, &mut used))
            });
        }
    }
    // Traverse anchors in file order, not HashMap iteration order.
    let mut wrappers = HashMap::new();
    let mut added = Vec::new();
    for node in &nodes {
        if let Some(groups) = anchored.remove(&node.id) {
            let wrapper = fresh(node.id, Uuid::nil(), WITH, &mut used);
            let children = groups
                .iter()
                .map(|group| group.id)
                .chain([node.id])
                .collect();
            wrappers.insert(node.id, wrapper);
            added.push(Node {
                id: wrapper,
                function: WITH_FUNCTION_ID,
                children: Some(children),
                ..Node::default()
            });
            added.extend(groups);
        }
    }
    for node in &mut nodes {
        for child in node.children.iter_mut().flatten() {
            if let Some(wrapper) = wrappers.get(child) {
                *child = *wrapper;
            }
        }
    }
    if let Some(wrapper) = wrappers.get(&root) {
        let position = added.iter().position(|node| node.id == *wrapper).unwrap();
        nodes.insert(0, added.remove(position));
    }
    nodes.extend(added);
    Ok(nodes)
}
