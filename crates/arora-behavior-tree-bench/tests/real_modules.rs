mod support;
use arora_behavior_tree_bench::{
    clock::Monotonic,
    forms::{Form, FORMS},
    measure,
    model::Workload,
};
use serde_json::{json, Value};
fn fixture() -> Value {
    json!({"keys":[{"name":"clock","initial":0.0}, {"name":"text","initial":"Hi"},
        {"name":"viseme","initial":""},{"name":"done","initial":false}],
        "text":"sequence\n  say(voice: \"\", text: {text}, viseme: {viseme})\n  unpack(values: f64[1], out_0: {done})\n",
        "clock":"clock","dt":0.02,"ticks":16,"warmUp":0,"events":[],"phases":{"names":["speech"],"runs":[[0,0]]}})
}

/// Missing bindings and literal mutable cells follow the text language's
/// contract; the benchmark imposes no additional module binding vocabulary.
#[test]
fn loader_diagnostics_reach_workload_errors() {
    for (text, message) in [
        ("missing::say()", "module"),
        ("missing_function()", "function"),
        ("say(missing: 0f64)", "parameter"),
        ("say(text: \"a\", text: \"b\")", "twice"),
        ("say(viseme: {viseme}, viseme: {viseme})", "twice"),
        ("say(text: 3f64)", "type str"),
        ("say(text: false)", "type str"),
        ("say(state: ())", "parameter"),
        ("running\n\nx: add(operand_0: @x)", "own"),
        (
            "running\n\nx: add(operand_0: @y)\ny: add(operand_0: @x)",
            "cycle",
        ),
        ("say(text:", "expected"),
    ] {
        let mut raw = fixture();
        raw["text"] = text.into();
        let error = Workload::parse(&raw.to_string())
            .err()
            .expect("must reject loader errors");
        assert!(
            error.contains(message) && error.contains("1:")
                || error.contains(message) && error.contains("3:"),
            "{error}"
        );
    }
    for text in [
        "say()",
        "say(text: {text})",
        "say(text: {text}, viseme: \"\")",
    ] {
        let mut raw = fixture();
        raw["text"] = text.into();
        assert!(Workload::parse(&raw.to_string()).is_ok());
    }
}

/// Module calls can be detached when the text leaves them outside the tree.
#[test]
fn real_functions_can_be_detached() {
    let mut raw = fixture();
    raw["text"] = "running\n\nspeech: {.status} = say(text: {text}, viseme: {viseme})".into();
    assert!(Workload::parse(&raw.to_string()).is_ok());
}

/// A sequence exposes the module's completion through a key, beyond agreement
/// between forms: neither an early nor a late Success may advance the sequence.
#[test]
fn say_timing_is_observed_through_the_tree() {
    let workload = Workload::parse(&fixture().to_string()).unwrap();
    for form in FORMS {
        let run = measure::replay(&workload, *form, true, &Monotonic::default()).unwrap();
        assert_eq!(run.calls, vec![1; 16], "{}", form.name());
        let expected = (0..16)
            .map(|tick| {
                use arora_types::value::Value;
                support::store_hash(vec![
                    ("clock", Some(Value::F64((tick + 1) as f64 * 0.02))),
                    ("text", Some(Value::String("Hi".into()))),
                    ("viseme", Some(Value::String("sil".into()))),
                    (
                        "done",
                        Some(if tick < 9 {
                            Value::Boolean(false)
                        } else {
                            Value::F64(1.0)
                        }),
                    ),
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
        let next = measure::replay(&workload, *form, true, &Monotonic::default()).unwrap();
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
        fn fixture_counts(&self, fixture: arora_behavior_tree_bench::forms::Fixture) -> (u64, u64) {
            FORMS[0].fixture_counts(fixture)
        }
        fn lower(
            &self,
            nodes: Nodes,
            resolver: &std::rc::Rc<VariableResolver<'static>>,
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
                // Without recording, the pilot and the measured replay are the
                // only replays, and both read this clock.
                measure::replay(&workload, form, false, &clock).unwrap();
                assert_eq!(clock.reads.get(), 34);
            });
        }
    });
}
