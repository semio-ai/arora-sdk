//! What an edit of one run costs a device that runs a main behavior and that
//! run: the `edit` call (`apply`) and the tick that lowers the edited graph.
//!
//! `cargo bench -p arora-behavior-tree --bench edit -- <edits> <mode>` runs
//! `<edits>` iterations after a warm-up, `<mode>` being `edit` (an edit, then
//! a tick) or `tick` (a tick alone). Each edit relinks one literal of the run,
//! the way a client relinks a run's ports.
//!
//! Count instructions, not time: run it under callgrind, or difference the
//! process's instructions retired (`/usr/bin/time -l` on macOS) between
//! `<edits>` and 0 iterations. The cost of an edit is an `edit` iteration
//! less a `tick` one. The time it prints is indicative only.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use arora_behavior::graph::{Graph, GraphDiff, Link, LinkSource, Node, Port};
use arora_behavior::{BehaviorContext, BehaviorInterpreter, RunPolicy, Status};
use arora_behavior_tree::behavior::{graph_type, BehaviorTreeInterpreter};
use arora_behavior_tree::nodes::{
    FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID, RUN_CALL_FUNCTION_ID, RUN_CALL_PARAM_ID,
    RUN_FUNCTION_ID, SEQ_FUNCTION_ID, SEQ_STAR_CURRENT_INDEX_PARAM_ID, SEQ_STAR_FUNCTION_ID,
    SUCCEED_FUNCTION_ID,
};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::value::Value;
use uuid::Uuid;

/// Registers callables and answers every module call `Running`.
#[derive(Default)]
struct Bridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next: u64,
}

impl CallBridge for Bridge {
    fn arora_call(&mut self, _call: Call) -> Result<CallResult, CallError> {
        Ok(CallResult {
            ret: Status::Running.into(),
            mutated: Vec::new(),
        })
    }
    fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
        self.next += 1;
        self.registered.insert(self.next, callable);
        CallableId { id: self.next }
    }
    fn arora_unregister_callable(&mut self, callable: &CallableId) {
        self.registered.remove(&callable.id);
    }
    fn arora_call_indirect(&mut self, callable: &CallableId) -> Result<Value, CallError> {
        let callable =
            self.registered
                .get(&callable.id)
                .cloned()
                .ok_or_else(|| CallError::Generic {
                    message: "unknown callable".to_string(),
                })?;
        callable.call(self)
    }
}

/// Allocates node ids.
struct Ids(u128);

impl Ids {
    fn next(&mut self) -> Uuid {
        self.0 += 1;
        Uuid::from_u128(self.0)
    }
}

fn control(graph: &mut Graph, id: Uuid, function: Uuid, children: Vec<Uuid>) {
    graph.nodes.insert(
        id,
        Node {
            id,
            function,
            children: Some(children),
            ..Node::default()
        },
    );
}

fn leaf(graph: &mut Graph, id: Uuid, function: Uuid) {
    graph.nodes.insert(
        id,
        Node {
            id,
            function,
            ..Node::default()
        },
    );
}

/// The link making the run-call leaf `node` call function `function`.
fn call_link(node: Uuid, function: u128) -> Link {
    let call = Call {
        module_id: Some(Uuid::from_u128(0xB0)),
        id: Uuid::from_u128(function),
        args: Vec::new(),
    };
    Link::new(
        Port::new(node, RUN_CALL_PARAM_ID),
        LinkSource::Literal(arora_types::value_serde::to_value(&call).unwrap()),
    )
}

/// A run-call leaf `node` calling function `function`.
fn call(graph: &mut Graph, node: Uuid, function: u128) {
    leaf(graph, node, RUN_CALL_FUNCTION_ID);
    graph.links.push(call_link(node, function));
}

/// The main behavior: a parallel of four `seq_star`s of five calls each.
fn main_behavior(ids: &mut Ids) -> Graph {
    let mut graph = Graph::empty();
    let root = ids.next();
    graph.root = Some(root);
    let mut branches = Vec::new();
    for _ in 0..4 {
        let branch = ids.next();
        let calls: Vec<Uuid> = (0..5).map(|_| ids.next()).collect();
        for (at, node) in calls.iter().enumerate() {
            call(&mut graph, *node, 0x100 + at as u128);
        }
        control(&mut graph, branch, SEQ_STAR_FUNCTION_ID, calls);
        graph.links.push(Link::new(
            Port::new(branch, SEQ_STAR_CURRENT_INDEX_PARAM_ID),
            LinkSource::Literal(Value::U16(0)),
        ));
        branches.push(branch);
    }
    control(&mut graph, root, PARALLEL_FUNCTION_ID, branches);
    graph
}

/// The run: `Parallel(Sequence(call, call, Fallback(Sequence(call, call),
/// Succeed)), Run)`, the shape of a stepping behavior. Returns it with the
/// call leaf an edit relinks.
fn run(ids: &mut Ids) -> (Graph, Uuid) {
    let mut graph = Graph::empty();
    let [root, step, first, second, or_else, guard, third, fourth, skip, run] =
        [(); 10].map(|_| ids.next());
    graph.root = Some(root);
    control(&mut graph, root, PARALLEL_FUNCTION_ID, vec![step, run]);
    control(
        &mut graph,
        step,
        SEQ_FUNCTION_ID,
        vec![first, second, or_else],
    );
    control(&mut graph, or_else, FALLBACK_FUNCTION_ID, vec![guard, skip]);
    control(&mut graph, guard, SEQ_FUNCTION_ID, vec![third, fourth]);
    for (at, node) in [first, second, third, fourth].into_iter().enumerate() {
        call(&mut graph, node, 0x200 + at as u128);
    }
    leaf(&mut graph, skip, SUCCEED_FUNCTION_ID);
    leaf(&mut graph, run, RUN_FUNCTION_ID);
    (graph, third)
}

fn main() {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--bench")
        .collect();
    let edits: u32 = args.first().map_or(1000, |n| n.parse().expect("<edits>"));
    let editing = match args.get(1).map(String::as_str) {
        None | Some("edit") => true,
        Some("tick") => false,
        Some(other) => panic!("<mode> is edit or tick, not {other}"),
    };

    let mut ids = Ids(0);
    let mut interp = BehaviorTreeInterpreter::new();
    interp.load(main_behavior(&mut ids)).unwrap();
    let (graph, relinked) = run(&mut ids);
    interp
        .spawn_graph(&graph_type(), graph, RunPolicy::Concurrent)
        .unwrap();
    let store = SimpleDataStore::new();
    let bridge = RefCell::new(Bridge::default());
    let functions = arora_behavior::FunctionIndex::new();
    let step = |interp: &mut BehaviorTreeInterpreter, edit: Option<u32>| {
        if let Some(edit) = edit {
            interp
                .apply(GraphDiff {
                    add_links: vec![call_link(relinked, 0x300 + u128::from(edit % 2))],
                    ..GraphDiff::default()
                })
                .unwrap();
        }
        let mut bridge = bridge.borrow_mut();
        let mut ctx = BehaviorContext {
            functions: &functions,
            store: &store,
            call_bridge: &mut *bridge,
        };
        interp.tick(&mut ctx).unwrap();
    };
    for warm in 0..10 {
        step(&mut interp, Some(warm));
    }

    let start = Instant::now();
    for edit in 0..edits {
        step(&mut interp, editing.then_some(edit));
    }
    let elapsed = start.elapsed();
    println!(
        "{} {} iteration(s): {:?} each (indicative)",
        edits,
        if editing { "edit" } else { "tick" },
        elapsed / edits.max(1)
    );
}
