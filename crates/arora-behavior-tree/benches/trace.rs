//! What a trace costs a tick: the same behavior ticked with no trace, with a
//! trace of statuses, and with a trace that also watches values.
//!
//! The behavior is a `Parallel` of eight branches, each
//! `Sequence(check, act × 4)`: 49 nodes, all reached while the first branch's
//! check succeeds. The first branch's check alternates between success and
//! failure every tick, so its four acts alternate between reached and not;
//! every other check succeeds. An act is a module leaf whose return value, a
//! number that changes at every call, is bound to a key of its own.
//!
//! `cargo bench -p arora-behavior-tree --bench trace -- <case> <ticks>` runs
//! `<ticks>` ticks after a first one, `<case>` being:
//!
//! - `none`: no trace;
//! - `statuses`: one trace with the default options, statuses and detached
//!   activity at every tick;
//! - `values`: one trace that also watches the first act of every branch;
//! - `both`: the two traces at once.
//!
//! Count instructions, not time: run it under callgrind, or difference the
//! process's instructions retired (`/usr/bin/time -l` on macOS) between
//! `<ticks>` and 0 ticks. The time it prints is indicative only.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use arora_behavior::graph::{Graph, Node, Source};
use arora_behavior::{
    BehaviorContext, BehaviorInterpreter, FunctionIndex, ModuleFunction, Status, TraceOptions,
};
use arora_behavior_tree::behavior::BehaviorTreeInterpreter;
use arora_behavior_tree::nodes::{PARALLEL_FUNCTION_ID, SEQ_FUNCTION_ID};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::DataStore;
use arora_types::record::module::frozen::Function;
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::Value;
use uuid::Uuid;

const MODULE: Uuid = Uuid::from_u128(0xB0);
/// `flip() -> Status`: success and failure in turn.
const FLIP: Uuid = Uuid::from_u128(0xF1);
/// `ok() -> Status`: success.
const OK: Uuid = Uuid::from_u128(0xF2);
/// `act() -> f64`: a number that changes at every call.
const ACT: Uuid = Uuid::from_u128(0xF3);
/// `_ret`'s id.
const RET: Uuid = Uuid::from_u128(0x5f726574_0000_4000_8000_000000000000);

const BRANCHES: u128 = 8;
const ACTS: u128 = 4;

fn function(id: Uuid, name: &str, return_ty: PrimitiveKind) -> (Uuid, ModuleFunction) {
    (
        id,
        ModuleFunction {
            module_id: MODULE,
            function_id: id,
            function_name: name.to_string(),
            function: Function {
                parameters: HashMap::new(),
                parameter_ordering: Vec::new(),
                return_ty: FrozenTy::from(return_ty),
            },
        },
    )
}

/// Registers callables and serves `flip`, `ok` and `act`.
#[derive(Default)]
struct Bridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next: u64,
    flips: Cell<u64>,
    acts: Cell<u64>,
}

impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        let ret = match call.id {
            FLIP => {
                self.flips.set(self.flips.get() + 1);
                if self.flips.get().is_multiple_of(2) {
                    Status::Success.into()
                } else {
                    Status::Failure.into()
                }
            }
            OK => Status::Success.into(),
            ACT => {
                self.acts.set(self.acts.get() + 1);
                Value::F64(self.acts.get() as f64)
            }
            id => return Err(CallError::FunctionNotFound { id }),
        };
        Ok(CallResult {
            ret,
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

fn act(branch: u128, at: u128) -> Uuid {
    Uuid::from_u128(0x1000 + branch * 0x10 + at)
}

/// The behavior, and the first act of every branch.
fn behavior() -> (Graph, Vec<Uuid>) {
    let root = Uuid::from_u128(1);
    let mut graph = Graph {
        root: Some(root),
        nodes: Vec::new(),
    };
    let mut branches = Vec::new();
    for branch in 0..BRANCHES {
        let sequence = Uuid::from_u128(0x100 + branch);
        let check = Uuid::from_u128(0x200 + branch);
        graph.nodes.push(Node {
            id: check,
            function: if branch == 0 { FLIP } else { OK },
            ..Node::default()
        });
        let acts: Vec<Uuid> = (0..ACTS).map(|at| act(branch, at)).collect();
        for (at, id) in acts.iter().enumerate() {
            graph.nodes.push(Node {
                id: *id,
                function: ACT,
                bind: [(RET, Source::Key(format!("out/{branch}/{at}")))].into(),
                children: None,
            });
        }
        graph.nodes.push(Node {
            id: sequence,
            function: SEQ_FUNCTION_ID,
            children: Some(std::iter::once(check).chain(acts).collect()),
            ..Node::default()
        });
        branches.push(sequence);
    }
    graph.nodes.push(Node {
        id: root,
        function: PARALLEL_FUNCTION_ID,
        children: Some(branches),
        ..Node::default()
    });
    let watched = (0..BRANCHES).map(|branch| act(branch, 0)).collect();
    (graph, watched)
}

fn main() {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--bench")
        .collect();
    let case = args.first().map_or("statuses", String::as_str);
    let ticks: u32 = args.get(1).map_or(10_000, |n| n.parse().expect("<ticks>"));

    let (graph, watched) = behavior();
    let mut interp = BehaviorTreeInterpreter::new();
    interp.load(graph).unwrap();
    let statuses = TraceOptions::default();
    let values = TraceOptions {
        values: watched,
        min_interval_ns: None,
    };
    let traces = match case {
        "none" => vec![],
        "statuses" => vec![statuses],
        "values" => vec![values],
        "both" => vec![statuses, values],
        other => panic!("<case> is none, statuses, values or both, not {other}"),
    };
    let handles: Vec<_> = traces
        .into_iter()
        .map(|options| interp.trace(options).unwrap())
        .collect();

    let store = SimpleDataStore::new();
    let mut bridge = Bridge::default();
    let functions: FunctionIndex = [
        function(FLIP, "flip", PrimitiveKind::U8),
        function(OK, "ok", PrimitiveKind::U8),
        function(ACT, "act", PrimitiveKind::F64),
    ]
    .into();
    let mut tick = |interp: &mut BehaviorTreeInterpreter| {
        let mut ctx = BehaviorContext {
            functions: &functions,
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();
    };
    tick(&mut interp);

    let start = Instant::now();
    for _ in 0..ticks {
        tick(&mut interp);
    }
    let elapsed = start.elapsed();
    for handle in &handles {
        assert!(
            store
                .read(std::slice::from_ref(&handle.feedback[0]))
                .remove(0)
                .is_some(),
            "the trace reported"
        );
    }
    println!(
        "{case}: {ticks} ticks, {:.0} ns/tick (indicative)",
        elapsed.as_nanos() as f64 / f64::from(ticks.max(1))
    );
}
