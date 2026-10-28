use arora_behavior_tree_bench::{
    clock::Monotonic,
    forms::{Form, FORMS},
    measure,
    model::Workload,
};
use serde_json::{json, Value};

fn say() -> Value {
    // Parameter order deliberately differs from the declaration.
    json!({"k":"call", "module":"silent-say", "op":"say",
        "args":[["voice", {"value":""}], ["text", {"key":1}]],
        "outputs":[["viseme",2]]})
}

fn fixture() -> Value {
    json!({"keys":[{"name":"clock","initial":0.0},
        {"name":"text","initial":"Hi"}, {"name":"viseme","initial":""},
        {"name":"done","initial":false}],
        "calls":[], "tree":{"k":"seq", "children":[say(),
            {"k":"call", "op":"set", "args":[["v", {"value":1.0}]],
             "outputs":[["v",3]], "state":0}]},
        "clock":0, "dt":0.02, "ticks":16, "warmUp":0, "events":[],
        "phases":{"names":["speech"], "runs":[[0,0]]}})
}

macro_rules! rejection {
    ($test:ident, $change:expr, $message:expr) => {
        #[test]
        fn $test() {
            let mut raw = fixture();
            let call = &mut raw["tree"]["children"][0];
            ($change)(call);
            let error = Workload::parse(&raw.to_string())
                .err()
                .expect("must reject");
            assert!(error.contains($message), "{error}");
        }
    };
}

rejection!(
    unknown_module,
    |c: &mut Value| c["module"] = "missing".into(),
    "unknown module"
);
rejection!(
    unknown_function,
    |c: &mut Value| c["op"] = "missing".into(),
    "unknown function"
);
rejection!(
    unknown_argument,
    |c: &mut Value| c["args"][0][0] = "missing".into(),
    "unknown parameter"
);
rejection!(
    unknown_output,
    |c: &mut Value| c["outputs"][0][0] = "missing".into(),
    "unknown parameter"
);
rejection!(
    duplicate_argument,
    |c: &mut Value| c["args"]
        .as_array_mut()
        .unwrap()
        .push(json!(["text", {"value":"Hi"}])),
    "bound twice: text"
);
rejection!(
    duplicate_output,
    |c: &mut Value| c["outputs"]
        .as_array_mut()
        .unwrap()
        .push(json!(["viseme", 2])),
    "bound twice: viseme"
);
rejection!(
    missing_argument,
    |c: &mut Value| {
        c["args"].as_array_mut().unwrap().pop();
    },
    "not bound: text"
);
rejection!(
    missing_output,
    |c: &mut Value| c["outputs"] = json!([]),
    "not bound: viseme"
);
rejection!(
    immutable_output,
    |c: &mut Value| {
        c["args"].as_array_mut().unwrap().pop();
        c["outputs"]
            .as_array_mut()
            .unwrap()
            .push(json!(["text", 1]));
    },
    "immutable parameter text"
);
rejection!(
    mutable_argument,
    |c: &mut Value| {
        c["outputs"] = json!([]);
        c["args"]
            .as_array_mut()
            .unwrap()
            .push(json!(["viseme", {"value":""}]));
    },
    "mutable parameter viseme"
);
rejection!(
    number_for_string,
    |c: &mut Value| c["args"][1][1] = json!({"value":3.0}),
    "literal kind does not match parameter text"
);
rejection!(
    boolean_for_string,
    |c: &mut Value| c["args"][1][1] = json!({"value":false}),
    "literal kind does not match parameter text"
);
rejection!(
    real_call_state,
    |c: &mut Value| c["state"] = 0.into(),
    "no state field"
);

#[test]
fn real_functions_are_tree_calls_only() {
    let mut raw = fixture();
    raw["calls"] = json!([{"module":"silent-say", "op":"first", "args":[["x", {"value":1.0}]], "publishes":null}]);
    let error = Workload::parse(&raw.to_string()).err().unwrap();
    assert!(error.contains("tree calls only"), "{error}");
}

/// A sequence exposes the module's completion through a key, beyond agreement
/// between forms: neither an early nor a late Success may advance the sequence.
#[test]
fn say_timing_is_observed_through_the_tree() {
    let workload = Workload::parse(&fixture().to_string()).unwrap();
    for form in FORMS {
        let run = measure::replay(&workload, *form, true, &Monotonic::default());
        assert_eq!(
            run.calls,
            [vec![1; 9], vec![2; 7]].concat(),
            "{}",
            form.name()
        );
        let expected = (0..16)
            .map(|tick| {
                measure::values_hash(&[
                    Some(arora_types::value::Value::String("sil".into())),
                    Some(if tick < 9 {
                        arora_types::value::Value::Boolean(false)
                    } else {
                        arora_types::value::Value::F64(1.0)
                    }),
                ])
            })
            .collect::<Vec<_>>();
        assert_eq!(run.written, expected, "{}", form.name());
    }
}

/// Repeated fresh devices must not inherit a sentence latched at Success.
#[test]
fn replays_reset_module_state_counts_and_digests() {
    let workload = Workload::parse(&fixture().to_string()).unwrap();
    for form in FORMS {
        let rows = measure::measure(&workload, &[*form], 3, &Monotonic::default()).unwrap();
        let first = &rows[0].runs[0];
        let next = measure::replay(&workload, *form, true, &Monotonic::default());
        measure::assert_counts(first, &next);
        assert_eq!(first.written, next.written);
    }
}

/// Slow clock reads expose overlap during ticking as well as lowering, even
/// when a speech replay would otherwise finish within one thread timeslice.
#[test]
fn parallel_replays_hold_one_device_through_lowering_and_ticks() {
    use arora_behavior::FunctionIndex;
    use arora_behavior_tree::{error::BehaviorTreeError, variable::VariableResolver, LoweredTree};
    use arora_behavior_tree_bench::forms::Nodes;
    use arora_types::call::CallBridge;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Barrier,
    };
    struct Exclusive(AtomicUsize);
    impl Form for Exclusive {
        fn name(&self) -> &'static str {
            "exclusive"
        }
        fn build(&self, w: &Workload) -> Nodes {
            FORMS[0].build(w)
        }
        fn fixture_counts(&self) -> [(u64, u64); 7] {
            FORMS[0].fixture_counts()
        }
        fn lower(
            &self,
            nodes: Nodes,
            resolver: &VariableResolver<'_>,
            functions: &FunctionIndex,
            bridge: &mut dyn CallBridge,
        ) -> Result<LoweredTree, BehaviorTreeError> {
            assert_eq!(
                self.0.fetch_add(1, Ordering::SeqCst),
                0,
                "overlapping devices"
            );
            FORMS[0].lower(nodes, resolver, functions, bridge)
        }
    }
    struct Clock<'a> {
        active: &'a AtomicUsize,
        reads: std::cell::Cell<usize>,
        monotonic: Monotonic,
    }
    impl arora_behavior_tree_bench::clock::Clock for Clock<'_> {
        fn now(&self) -> f64 {
            let read = self.reads.get() + 1;
            self.reads.set(read);
            // Two reads for the pilot, then two per tick for the measured
            // replay. Keep each replay active across its clock reads.
            if read == 1 || read == 3 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(self.active.load(Ordering::SeqCst), 1);
            if read == 2 || read == 34 {
                self.active.fetch_sub(1, Ordering::SeqCst);
            }
            arora_behavior_tree_bench::clock::Clock::now(&self.monotonic)
        }
    }
    let form = Exclusive(AtomicUsize::new(0));
    let barrier = Barrier::new(8);
    let raw = fixture().to_string();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let (form, barrier, raw) = (&form, &barrier, &raw);
            scope.spawn(move || {
                let workload = Workload::parse(raw).unwrap();
                barrier.wait();
                let clock = Clock {
                    active: &form.0,
                    reads: std::cell::Cell::new(0),
                    monotonic: Monotonic::default(),
                };
                measure::replay(&workload, form, true, &clock);
                assert_eq!(clock.reads.get(), 34);
            });
        }
    });
}
