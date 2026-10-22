//! Load-time placement of detached computations before their readers.
//!
//! Placement preserves port references and makes each used detached node an
//! attached `_on_change` leaf, unless its arguments contain a call or it
//! publishes a key that a sink staying detached also publishes: those nodes
//! remain detached ([`place_detached`] says why). Store-bound outputs keep
//! private ports and publish through native `Copy` nodes after their
//! computation: a tree node reading its key sees the new value in the same
//! tick, instead of the next tick. Sinks run before the root rather than after
//! it. Stateful functions, writes during a tick, errors, and store-key
//! feedback therefore need not behave like demand-driven detached evaluation.
use std::collections::{HashMap, HashSet};

use arora_types::value::Value;
use uuid::Uuid;

use crate::error::BehaviorTreeError;
use crate::nodes::{
    COPY_FUNCTION_ID, COPY_OUT_PARAM_ID, COPY_VALUE_PARAM_ID, PARALLEL_FUNCTION_ID,
    WITH_FUNCTION_ID,
};
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
const INPUT: u128 = 0x494e5000000040008000000000000000;
const PUBLICATIONS: u128 = 0x50554247000040008000000000000000;
const PUBLISH: u128 = 0x50554200000040008000000000000000;
const PORT: u128 = 0x504f5200000040008000000000000000;

// The incoming child index distinguishes repeated children of one parent.
type ReaderPath = Vec<(Uuid, usize)>;

/// A store key: the name of the variable that names it, which two variables
/// may share, or the cell itself when no named variable holds it.
#[derive(PartialEq, Eq, Hash)]
enum StoreKey {
    Named(String),
    Cell(usize),
}

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
/// A final root prerequisite publishes placed sinks in file order from their
/// private ports, after all prerequisite computations and before the body;
/// each sink's outputs publish in detached publication's order, its return
/// first, then its mutable parameters in signature order.
/// Every path of a shared reader contributes to the common ancestor. A root
/// reader or any sink forces a root anchor. An input a detached node's output
/// feeds reads a fresh tree-local variable in place of the store key its
/// argument names, as it reads a private cell when detached.
/// Inert loose controls are retained. Generated ids and list order are stable,
/// including when an existing id collides with a generated one.
///
/// A detached node stays detached, unchanged in the file, when:
///
/// - nothing uses it, so the detached layer never evaluates it;
/// - its arguments contain a call: the incremental executor never caches a
///   node with a nested call, which keeps call-site state, so placed it would
///   be called at every tick, while detached its call is cached by argument
///   value;
/// - it is a sink publishing a key that a sink staying detached also
///   publishes, and so on until no placed sink shares a key with one: a
///   detached sink publishes after the tree and a placed one before the body,
///   so the publishers of one key stay together and publish in file order.
///   Two variables of one name name one key.
///
/// Every other used node joins a group.
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
    let mut active: HashSet<usize> = (0..detached.len())
        .filter(|at| {
            !analysis.unused.contains(&detached[*at].id)
                && !crate::incremental::has_call(&detached[*at])
        })
        .collect();
    // A detached sink publishes after the tree, and a placed one before the
    // body. A sink sharing a key with a sink that stays detached stays too, so
    // the publishers of one key all publish after the tree, in file order.
    let named: HashMap<usize, String> = tree
        .variables
        .borrow()
        .iter()
        .filter_map(|(id, cell)| Some((cell.identity(), names.get(id)?.clone())))
        .collect();
    let keys = |at: usize| {
        analysis.publications[at].iter().map(|(_, cell)| {
            named
                .get(&cell.identity())
                .map_or(StoreKey::Cell(cell.identity()), |name| {
                    StoreKey::Named(name.clone())
                })
        })
    };
    let mut retained: HashSet<StoreKey> = analysis
        .sinks
        .iter()
        .filter(|at| !active.contains(at))
        .flat_map(|at| keys(*at))
        .collect();
    while let Some(at) = analysis
        .sinks
        .iter()
        .copied()
        .find(|at| active.contains(at) && keys(*at).any(|key| retained.contains(&key)))
    {
        active.remove(&at);
        retained.extend(keys(at));
    }
    // Keep dependencies across retained detached calls: a placed consumer
    // pulls those calls, so their placed inputs must already have executed.
    let mut dependencies = vec![HashSet::new(); detached.len()];
    for at in &analysis.order {
        for read in &analysis.reads[*at] {
            dependencies[*at].insert(*read);
            let upstream = dependencies[*read].clone();
            dependencies[*at].extend(upstream);
        }
    }
    let mut edges = vec![Vec::new(); detached.len()];
    for reader in 0..detached.len() {
        if active.contains(&reader) {
            for read in &dependencies[reader] {
                if active.contains(read) {
                    edges[reader].push(*read);
                    edges[*read].push(reader);
                }
            }
        }
    }
    let mut used: HashSet<Uuid> = nodes
        .iter()
        .map(|node| node.id)
        .chain(names.keys().copied())
        .chain(tree.variables.borrow().keys().copied())
        .collect();
    // An input a detached node's output feeds reads that output's cell, even
    // when its argument names a store key: loading gives the two a private
    // cell only while the writer is detached. A fresh tree-local variable
    // keeps the input, and the writer with it, off the key once the writer is
    // attached. Whether an input is fed comes from the detached analysis: a
    // detached input that names a key and is not fed reads the key.
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
            if !matches!(stored, Some(VariableCell::Stored(_))) || !analysis.fed.contains(&port) {
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
    let placeable: HashMap<Uuid, usize> = active.iter().map(|at| (detached[*at].id, *at)).collect();
    let mut publications: HashMap<Uuid, Vec<Node>> = HashMap::new();
    let mut inputs: HashMap<Uuid, Vec<Node>> = HashMap::new();
    for node in &mut nodes {
        let Some(at) = placeable.get(&node.id) else {
            continue;
        };
        // Detached publication's order: of two outputs bound to one key, the
        // later one's value stands.
        for (parameter, _) in &analysis.publications[*at] {
            let parameter = *parameter;
            let port = NodeParameterId {
                node: node.id,
                parameter,
            };
            let target = node
                .arguments
                .insert(
                    parameter,
                    Expression::VariableId(fresh(node.id, parameter, PORT, &mut used)),
                )
                .unwrap();
            // Mutable store-backed state starts from the live key, as it
            // does when evaluated by the detached layer. Returns only publish.
            if parameter != _RET_PARAM_ID {
                inputs.entry(node.id).or_default().push(Node {
                    id: fresh(node.id, parameter, INPUT, &mut used),
                    function: COPY_FUNCTION_ID,
                    arguments: HashMap::from([
                        (COPY_VALUE_PARAM_ID, target.clone()),
                        (COPY_OUT_PARAM_ID, node.arguments[&parameter].clone()),
                    ]),
                    children: None,
                });
            }
            publications.entry(node.id).or_default().push(Node {
                id: fresh(node.id, parameter, PUBLISH, &mut used),
                function: COPY_FUNCTION_ID,
                arguments: HashMap::from([
                    (COPY_VALUE_PARAM_ID, Expression::NodeArgument(port)),
                    (COPY_OUT_PARAM_ID, target),
                ]),
                children: None,
            });
        }
    }
    let mut seen = HashSet::new();
    let mut anchored: HashMap<Uuid, Vec<Node>> = HashMap::new();
    for first in 0..detached.len() {
        if !active.contains(&first) || seen.contains(&first) {
            continue;
        }
        let mut group = HashSet::new();
        let mut stack = vec![first];
        while let Some(at) = stack.pop() {
            if group.insert(at) {
                stack.extend(edges[at].iter().copied().filter(|at| active.contains(at)));
            }
        }
        seen.extend(group.iter().copied());
        let reads_group = |at: &usize| {
            group.contains(at) || dependencies[*at].iter().any(|read| group.contains(read))
        };
        let mut common: Option<ReaderPath> = None;
        for (reader, pulls) in &analysis.pulls {
            if !pulls.iter().any(reads_group) {
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
        let anchor = if scope == PlacementScope::Root || analysis.sinks.iter().any(reads_group) {
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
            .flat_map(|at| {
                let id = detached[*at].id;
                inputs
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .map(|node| node.id)
                    .chain(std::iter::once(id))
            })
            .collect();
        anchored.entry(anchor).or_default().push(Node {
            id: fresh(anchor, detached[first].id, GROUP, &mut used),
            function: PARALLEL_FUNCTION_ID,
            children: Some(children),
            ..Node::default()
        });
    }

    // All sink computations finish before any key is published. Publication
    // follows file order even when components interleave or data order differs.
    let publication_ids: Vec<_> = nodes
        .iter()
        .flat_map(|node| {
            publications
                .get(&node.id)
                .into_iter()
                .flatten()
                .map(|node| node.id)
        })
        .collect();
    if !publication_ids.is_empty() {
        anchored.entry(root).or_default().push(Node {
            id: fresh(root, Uuid::nil(), PUBLICATIONS, &mut used),
            function: PARALLEL_FUNCTION_ID,
            children: Some(publication_ids),
            ..Node::default()
        });
    }

    for node in &mut nodes {
        if placeable.contains_key(&node.id) {
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
        if let Some(inputs) = inputs.remove(&node.id) {
            added.extend(inputs);
        }
        if let Some(publications) = publications.remove(&node.id) {
            added.extend(publications);
        }
    }
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
