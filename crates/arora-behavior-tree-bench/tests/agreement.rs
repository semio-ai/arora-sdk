mod support;
use arora_behavior_tree_bench::{
    clock::Monotonic,
    forms::{self, Fixture, FORMS},
    measure,
    model::Workload,
    scale::Scenario,
    synthetic,
};

#[test]
fn every_registered_form_agrees_and_meets_its_count_contract() {
    let fixtures = [Fixture::Synthetic, Fixture::Conversation]
        .into_iter()
        .chain(Scenario::ALL.map(Fixture::Scale));
    for (index, fixture) in fixtures.enumerate() {
        let workload = fixture.workload();
        let measurements = measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap();
        for (form, m) in FORMS.iter().zip(measurements) {
            assert_eq!(
                m.differing,
                0,
                "fixture {index}: {} writes different values",
                form.name()
            );
            let counts: (u64, u64) = (
                m.runs[0].calls.iter().sum(),
                m.runs[0].dispatches.iter().sum(),
            );
            assert_eq!(
                counts,
                form.fixture_counts(fixture),
                "fixture {index}: {} count contract",
                form.name()
            );
        }
    }
}

#[test]
fn selection_keeps_the_detached_agreement_reference() {
    let workload = synthetic::workload();
    let forms = forms::selected(Some("attached-naive")).unwrap();
    let rows = measure::measure(&workload, &forms, 1, &Monotonic::default()).unwrap();
    assert_eq!(rows[0].differing, 0);
    assert!(forms::selected(Some("typo")).is_err());
    assert!(forms::selected(Some("detached,detached")).is_err());
    assert!(measure::measure(&workload, FORMS, 0, &Monotonic::default()).is_err());
}

#[test]
fn parser_rejects_invalid_envelope() {
    for (field, value, message) in [
        ("warmUp", serde_json::json!(160), "warmUp"),
        ("ticks", serde_json::json!(0), "ticks"),
        ("dt", serde_json::json!(0), "dt"),
        ("clock", serde_json::json!("unknown"), "unknown key"),
        (
            "events",
            serde_json::json!([[0, "missing", 0]]),
            "unknown key",
        ),
        ("events", serde_json::json!([[160, "input", 0]]), "tick"),
        (
            "phases",
            serde_json::json!({"names":[],"runs":[]}),
            "phase names",
        ),
        (
            "phases",
            serde_json::json!({"names":["x"],"runs":[[0,1]]}),
            "phase run",
        ),
        (
            "phases",
            serde_json::json!({"names":[""],"runs":[]}),
            "phase names",
        ),
        (
            "phases",
            serde_json::json!({"names":["a,b"],"runs":[]}),
            "phase names",
        ),
        (
            "phases",
            serde_json::json!({"names":["a;b"],"runs":[]}),
            "phase names",
        ),
        (
            "phases",
            serde_json::json!({"names":["x"],"runs":[[160,0]]}),
            "phase run",
        ),
        (
            "phases",
            serde_json::json!({"names":["x"],"runs":[[2,0],[1,0]]}),
            "phase run",
        ),
    ] {
        let mut raw = synthetic::json();
        raw[field] = value;
        assert!(Workload::parse(&raw.to_string())
            .err()
            .unwrap()
            .contains(message));
    }
    for name in ["", "input", ".target"] {
        let mut raw = synthetic::json();
        raw["keys"][3]["name"] = name.into();
        assert!(Workload::parse(&raw.to_string()).is_err());
    }
}
#[test]
fn parser_rejects_shared_outputs_but_accepts_loader_warnings() {
    let mut raw = synthetic::json();
    raw["text"] = "unpack(values: f32[1,0], out_0: {target}, out_1: {target})".into();
    let error = Workload::parse(&raw.to_string()).err().unwrap();
    assert!(error.contains("two of Unpack's ports"), "{error}");
    raw["text"] = "pulse(out: {target}, state: {target})".into();
    assert!(Workload::parse(&raw.to_string())
        .err()
        .unwrap()
        .contains("at most one output"));
    raw["text"] = "parallel\n  unpack(values: f32[1], out_0: {target})\n  unpack(values: f32[2], out_0: {target})".into();
    assert!(Workload::parse(&raw.to_string()).is_ok());
}

#[test]
fn summary_excludes_warm_up_and_uses_tick_medians_then_linear_quantiles() {
    let mut workload = synthetic::workload();
    workload.warm_up = 1;
    workload.events.truncate(4);
    workload.phases = vec![0, 0, 1, 1];
    workload.phase_names.truncate(2);
    let replay = |times| measure::Replay {
        times,
        calls: vec![99, 1, 2, 3],
        dispatches: vec![99, 4, 5, 6],
        written: Vec::new(),
        repetitions: 1,
        replay_times: Vec::new(),
        timing_mode: measure::TimingMode::Tick,
        replay_us: Vec::new(),
        mean_us: 0.0,
        step_us: 0.0,
    };
    let runs = vec![
        replay(vec![100.0, 1.0, 2.0, 3.0]),
        replay(vec![100.0, 20.0, 1.0, 5.0]),
        replay(vec![100.0, 3.0, 4.0, 9.0]),
    ];
    let s = measure::summarize(&workload, &runs);
    assert_eq!(s.mean, 10.0 / 3.0);
    assert_eq!(s.p50, Some(3.0));
    assert!((s.p95.unwrap() - 4.8).abs() < 1e-12);
    assert_eq!(s.calls, 2.0);
    assert_eq!(s.dispatches, 5.0);
    assert_eq!(s.phases, vec![Some(3.0), Some(3.5)]);
}

fn write_hash(tick: usize, gate: bool, out: f64) -> String {
    use arora_types::value::Value;
    support::store_hash(vec![
        ("clock", Some(Value::F64((tick + 1) as f64 / 60.0))),
        ("gate", Some(Value::Boolean(gate))),
        ("observed", Some(Value::F64(0.0))),
        ("out", Some(Value::F64(out))),
    ])
}

// The store must win over the host's last write when the tree writes the key.
fn write_raw() -> serde_json::Value {
    let text = "fallback\n  sequence\n    equal(a: {gate}, b: true)\n    unpack(values: f64[2], out_0: {out})\n  running";
    serde_json::json!({"keys":[{"name":"clock","initial":0.0},{"name":"out","initial":0.0},{"name":"gate","initial":true},{"name":"observed","initial":0.0}],
        "text":text,"ticks":3,"warmUp":0,"clock":"clock","dt":1.0/60.0,
        "events":[[0,"out",1.0],[1,"gate",false],[2,"out",1.0]],"phases":{"names":["one"],"runs":[]}})
}
#[test]
fn host_reapplies_events_after_tree_writes() {
    let workload = Workload::parse(&write_raw().to_string()).unwrap();
    for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
        let expected: Vec<_> = [2.0, 2.0, 1.0]
            .into_iter()
            .enumerate()
            .map(|(tick, x)| write_hash(tick, tick == 0, x))
            .collect();
        assert_eq!(row.runs[0].written, expected, "{}", row.form);
    }
}

#[test]
fn digests_of_equal_composites_agree() {
    use arora_types::keyvalue::{KeyValue, KeyValueField};
    use arora_types::value::Value;
    // Two maps holding the same fields iterate them in different orders.
    let record = |names: Vec<usize>| {
        Value::KeyValue(KeyValue {
            id: uuid::Uuid::nil(),
            fields: names
                .into_iter()
                .map(|i| {
                    let name = format!("field{i}");
                    let field = KeyValueField {
                        id: uuid::Uuid::from_u128(i as u128),
                        name: name.clone(),
                        value: Some(Box::new(Value::F32(i as f32))),
                    };
                    (name, field)
                })
                .collect(),
        })
    };
    assert_eq!(
        measure::values_hash(&[Some(record((0..32).collect()))]),
        measure::values_hash(&[Some(record((0..32).rev().collect()))])
    );
    assert_eq!(
        measure::values_hash(&[Some(Value::ArrayF32(vec![-0.0, 1.0]))]),
        measure::values_hash(&[Some(Value::ArrayF32(vec![0.0, 1.0]))])
    );
    assert_ne!(
        measure::values_hash(&[Some(Value::ArrayF32(vec![0.0, 1.0]))]),
        measure::values_hash(&[Some(Value::ArrayF32(vec![1.0, 0.0]))])
    );
}

#[test]
fn digests_canonicalize_signed_zero_in_both_widths() {
    use arora_types::value::Value;
    assert_eq!(
        measure::values_hash(&[Some(Value::F32(-0.0))]),
        measure::values_hash(&[Some(Value::F32(0.0))])
    );
    assert_eq!(
        measure::values_hash(&[Some(Value::F64(-0.0))]),
        measure::values_hash(&[Some(Value::F64(0.0))])
    );
}
#[test]
fn tree_written_values_change_the_digest() {
    let mut raw = write_raw();
    let workload = Workload::parse(&raw.to_string()).unwrap();
    let first = measure::replay(&workload, FORMS[0], true, &Monotonic::default()).unwrap();
    raw["text"] = raw["text"]
        .as_str()
        .unwrap()
        .replace("f64[2]", "f64[3]")
        .into();
    let workload = Workload::parse(&raw.to_string()).unwrap();
    let second = measure::replay(&workload, FORMS[0], true, &Monotonic::default()).unwrap();
    assert_ne!(first.written[0], second.written[0]);
}

#[test]
fn timed_actions_progress_at_thirty_hz() {
    for text in [
        "sequence\n  hold(t: {clock}, duration: 0.05, state: ())\n  unpack(values: f64[2], out_0: {out})",
        "unpack(values: @frame, out_0: {out})\n\nlate: greaterthan(lhs: {clock}, rhs: 0.09f32)\nframe: if(cond: @late, then: f64[2], else: f64[1])",
    ] {
        let mut raw = write_raw();
        raw["text"] = text.into(); raw["dt"] = (1.0/30.0).into(); raw["events"] = serde_json::json!([]);
        let workload = Workload::parse(&raw.to_string()).unwrap();
        for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
            let expected = vec![
                ("clock", Some(arora_types::value::Value::F64(0.1))),
                ("gate", Some(arora_types::value::Value::Boolean(true))),
                ("observed", Some(arora_types::value::Value::F64(0.0))),
                ("out", Some(arora_types::value::Value::F64(2.0))),
            ];
            assert_eq!(row.runs[0].written[2], support::store_hash(expected));
            assert_eq!(row.differing,0);
        }
    }
}
/// Native frame selection leaves only the frame due at the device time.
#[test]
fn a_tree_call_leaves_its_last_write_to_an_output() {
    let mut raw = write_raw();
    raw["text"] = "unpack(values: @frame, out_0: {out})\n\ndue: greaterthan(lhs: {clock}, rhs: 0f32)\nframe: if(cond: @due, then: f64[0], else: f64[1])".into();
    raw["events"] = serde_json::json!([]);
    let workload = Workload::parse(&raw.to_string()).unwrap();
    for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
        assert_eq!(row.runs[0].written[0], write_hash(0, true, 0.0));
    }
}

/// By hand: each tick has one hold and one say. The detached constant runs
/// once; the two native unpacks demand its same value. The nested parallel
/// has six indirect ticks (root, hold, inner parallel, two unpacks, say).
/// Attached adds its wrapper and its graph leaf, and runs the constant each tick.
#[test]
fn counting_includes_reentrant_dataflow_host_and_real_calls_once() {
    let mut raw = write_raw();
    raw["text"] = "parallel\n  hold(t: {clock}, duration: 0, state: ())\n  parallel\n    unpack(values: @values, out_0: {out})\n    unpack(values: @values, out_0: {observed})\n  say(text: \"\", voice: \"\", viseme: {viseme})\n\nvalues: constant(param.value: f64[3])".into();
    raw["keys"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"name":"viseme","initial":""}));
    raw["events"] = serde_json::json!([]);
    let workload = Workload::parse(&raw.to_string()).unwrap();
    for form in FORMS {
        let run = measure::replay(&workload, *form, true, &Monotonic::default()).unwrap();
        let detached = form.name() == "detached";
        assert_eq!(
            run.calls,
            if detached {
                vec![3, 2, 2]
            } else {
                vec![3, 3, 3]
            }
        );
        assert_eq!(run.dispatches, vec![if detached { 6 } else { 8 }; 3]);
    }
}

/// A backwards reference and a graph reading a tree output must be ordered
/// around the tree; the naive form still runs even an unused graph every tick.
#[test]
fn attached_transform_orders_calls_and_replaces_return_ports() {
    let mut raw = write_raw();
    raw["text"] = "unpack(values: f64[4], out_0: {out})\n\nlast: {observed} = add(operand_0: @first)\nfirst: add(operand_0: {out}, operand_1: 1f32)\nunused: add(operand_0: 9f32)".into();
    raw["events"] = serde_json::json!([]);
    let workload = Workload::parse(&raw.to_string()).unwrap();
    let rows = measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap();
    assert!(rows.iter().all(|r| r.differing == 0));
    assert_eq!(rows[1].runs[0].calls, vec![3; 3]);
    let nodes = FORMS[1].build(&workload);
    for node in &nodes.0 {
        assert!(node
            .arguments
            .values()
            .all(|e| !matches!(e, arora_behavior_tree::schema::Expression::NodeArgument(_))));
    }
}

/// A state port read through a selector shares the attached call's private
/// cell, while a text-only output key still participates in agreement.
#[test]
fn attached_transform_preserves_state_ports_and_text_only_keys() {
    let mut raw = write_raw();
    raw["text"] = "{introduced} = add(operand_0: @smooth.state[.0])\n\nsmooth: damp(in: f32[1,2], dt: 0.1f32, param.half_life: 0.1f32, state: ())".into();
    raw["events"] = serde_json::json!([]);
    let workload = Workload::parse(&raw.to_string()).unwrap();
    let rows = measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap();
    assert!(rows.iter().all(|r| r.differing == 0));
    use arora_types::value::Value;
    let expected = (0..3)
        .map(|tick| {
            support::store_hash(vec![
                ("clock", Some(Value::F64((tick + 1) as f64 / 60.0))),
                ("gate", Some(Value::Boolean(true))),
                ("observed", Some(Value::F64(0.0))),
                ("out", Some(Value::F64(0.0))),
                ("introduced", Some(Value::F32(1.0))),
            ])
        })
        .collect::<Vec<_>>();
    for row in rows {
        assert_eq!(row.runs[0].written, expected);
    }
}

/// Scatter's runtime key resolver writes names outside the envelope. Its
/// writes enter the digest, and an event must restore a key it overwrote.
#[test]
fn scatter_writes_dynamic_keys_and_host_events_remain_authoritative() {
    use arora_types::value::Value;
    let mut raw = write_raw();
    raw["text"] = "fallback\n  sequence\n    equal(a: {gate}, b: true)\n    scatter(keys: str[\"out\",\"extra\"], values: f64[2,3])\n  running".into();
    let workload = Workload::parse(&raw.to_string()).unwrap();
    let expected = (0..3)
        .map(|tick| {
            let entries = [
                ("clock", Value::F64((tick + 1) as f64 / 60.0)),
                ("extra", Value::F64(3.0)),
                ("gate", Value::Boolean(tick == 0)),
                ("observed", Value::F64(0.0)),
                ("out", Value::F64(if tick == 2 { 1.0 } else { 2.0 })),
            ];
            measure::values_hash(
                &entries
                    .into_iter()
                    .flat_map(|(name, value)| [Some(Value::String(name.into())), Some(value)])
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
        assert_eq!(row.differing, 0);
        assert_eq!(row.runs[0].written, expected);
    }
}

/// These shapes depend on detached demand or publication order; agreement must
/// expose their different stores, including writes through selection aliases.
#[test]
fn order_dependent_workloads_report_differing_ticks() {
    for (name, source) in [
        (
            "port read after tree write",
            include_str!("fixtures/port_read_after_tree_write.json"),
        ),
        (
            "host event on graph key",
            include_str!("fixtures/host_event_on_sink_key.json"),
        ),
        (
            "selection alias",
            include_str!("fixtures/tree_write_via_select_alias.json"),
        ),
        (
            "graph literal input",
            include_str!("fixtures/tree_writes_graph_literal_input.json"),
        ),
        (
            "scatter graph key",
            include_str!("fixtures/scatter_and_graph_write_one_key.json"),
        ),
        (
            "escaped selection",
            include_str!("fixtures/selection_alias_after_tree_write.json"),
        ),
        (
            "graph key input",
            include_str!("fixtures/tree_writes_graph_key_input.json"),
        ),
        (
            "scatter clock reader",
            include_str!("fixtures/scatter_with_clock_reader.json"),
        ),
    ] {
        let workload = Workload::parse(source).unwrap_or_else(|e| panic!("{name}: {e}"));
        let rows = measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap();
        assert_eq!(rows[0].differing, 0, "{name}");
        assert!(rows[1].differing > 0, "{name}: silent agreement");
    }
}

/// A never-run detached native and a tree reading a graph's local cell are
/// valid runtime workloads; their order is judged by replay, not parsing.
#[test]
fn runtime_accepts_local_graph_cells_and_unexecuted_natives() {
    for source in [
        include_str!("fixtures/tree_reads_local_cell_of_graph.json"),
        include_str!("fixtures/never_running_native_plus_tree_writer.json"),
    ] {
        let workload = Workload::parse(source).unwrap();
        let rows = measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap();
        assert!(rows.iter().all(|r| r.differing == 0));
    }
}

/// Repeated events restore an aliased cell before every tick, observable as
/// seen=1 even though the tree leaves k=5 at the end of the tick.
#[test]
fn host_reapplies_events_on_selection_aliases() {
    use arora_types::value::Value;
    for source in [
        include_str!("fixtures/events_on_selection_alias.json"),
        include_str!("fixtures/events_on_input_alias.json"),
    ] {
        let workload = Workload::parse(source).unwrap();
        let expected = (0..4)
            .map(|tick| {
                support::store_hash(vec![
                    ("clock", Some(Value::F64((tick + 1) as f64 * workload.dt))),
                    ("k", Some(Value::F32(5.0))),
                    ("seen", Some(Value::F64(1.0))),
                ])
            })
            .collect::<Vec<_>>();
        for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
            assert_eq!(row.differing, 0);
            assert_eq!(row.runs[0].written, expected, "{}", row.form);
        }
    }
}

/// A runtime-only lowering failure must return a diagnostic naming the failing form,
/// including when the selected forms omit the reference.
#[test]
fn lowering_errors_name_the_form_without_panicking() {
    let workload =
        Workload::parse(include_str!("fixtures/graph_state_bound_to_select.json")).unwrap();
    for form in FORMS {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            measure::measure(&workload, &[*form], 1, &Monotonic::default())
        }));
        let error = outcome
            .expect("lowering must not panic")
            .err()
            .expect("lowering must fail");
        assert!(error.starts_with("detached:"), "{error}");
    }
}

#[test]
fn a_malformed_saved_state_is_a_tick_error() {
    let mut raw = synthetic::json();
    raw["text"] =
        "hold(t: {arora/time}, duration: 0, value: 1, out: {target}, state: f64[])".into();
    let workload = Workload::parse(&raw.to_string()).unwrap();
    for form in FORMS {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            measure::measure(&workload, &[*form], 1, &Monotonic::default())
        }));
        let error = outcome
            .expect("a malformed state must not panic")
            .err()
            .expect("a malformed state must fail");
        assert!(
            error.contains("a saved state holds 3 numbers, not 0"),
            "{error}"
        );
    }
    raw["text"] =
        "hold(t: {arora/time}, duration: 0, value: 1, out: {target}, state: \"x\")".into();
    let workload = Workload::parse(&raw.to_string()).unwrap();
    for form in FORMS {
        let error = measure::measure(&workload, &[*form], 1, &Monotonic::default())
            .err()
            .expect("a state that is not () must fail");
        assert!(error.contains("a saved state starts as ()"), "{error}");
    }
}

/// NaN payloads/signs cannot create disagreements for equal float values,
/// including specialized float arrays and heterogeneous arrays.
#[test]
fn digests_canonicalize_nan_in_both_widths_and_arrays() {
    use arora_types::value::Value;
    let a32 = f32::from_bits(0x7fc0_0001);
    let b32 = f32::from_bits(0xffc0_0002);
    let a64 = f64::from_bits(0x7ff8_0000_0000_0001);
    let b64 = f64::from_bits(0xfff8_0000_0000_0002);
    for (a, b) in [
        (Value::F32(a32), Value::F32(b32)),
        (Value::F64(a64), Value::F64(b64)),
        (Value::ArrayF32(vec![a32]), Value::ArrayF32(vec![b32])),
        (Value::ArrayF64(vec![a64]), Value::ArrayF64(vec![b64])),
        (
            Value::ArrayValue(vec![Value::F32(a32), Value::F64(a64)]),
            Value::ArrayValue(vec![Value::F32(b32), Value::F64(b64)]),
        ),
    ] {
        assert_eq!(
            measure::values_hash(&[Some(a)]),
            measure::values_hash(&[Some(b)])
        );
    }
}

/// Distinct structure types with identical fields remain distinct in a digest.
#[test]
fn digests_include_structure_ids() {
    use arora_types::value::{Structure, Value};
    let value = |id| Some(Value::Structure(Structure { id, fields: vec![] }));
    assert_ne!(
        measure::values_hash(&[value(uuid::Uuid::from_u128(1))]),
        measure::values_hash(&[value(uuid::Uuid::from_u128(2))]),
    );
}

/// Keys without writers and absent cells are part of agreement, independent
/// of envelope order. Text-only keys also enter through runtime resolution.
#[test]
fn digests_cover_all_store_names_values_and_absent_cells() {
    use arora_types::value::Value;
    let mut raw = write_raw();
    raw["text"] = "reader: equal(a: {text_only}, b: 1f64)".into();
    raw["events"] = serde_json::json!([]);
    raw["keys"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"name":"absent","initial":null}));
    let expected = (0..3)
        .map(|tick| {
            support::store_hash(vec![
                ("clock", Some(Value::F64((tick + 1) as f64 / 60.0))),
                ("gate", Some(Value::Boolean(true))),
                ("observed", Some(Value::F64(0.0))),
                ("out", Some(Value::F64(0.0))),
                ("absent", None),
                ("text_only", None),
            ])
        })
        .collect::<Vec<_>>();
    for _ in 0..2 {
        let workload = Workload::parse(&raw.to_string()).unwrap();
        for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
            assert_eq!(row.runs[0].written, expected);
        }
        raw["keys"].as_array_mut().unwrap().reverse();
    }
}

/// An escaped selection write must be observed even when the workload has
/// another directly written key; digest coverage cannot come from writer scans.
#[test]
fn whole_store_detects_selection_alias_escape() {
    let workload = Workload::parse(include_str!(
        "fixtures/selection_alias_with_another_output.json"
    ))
    .unwrap();
    let rows = measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap();
    assert_eq!(rows[0].differing, 0);
    assert!(rows[1].differing > 0);
}

/// The direct-output sanity check deliberately leaves input aliases to the
/// runtime, including duplicate cells that are not direct VariableId bindings.
#[test]
fn parser_leaves_output_alias_resolution_to_the_runtime() {
    for text in [
        "parallel\n  reader: equal(a: {target}, b: 1f64)\n  pulse(out: {target}, state: @reader.a)",
        "running\n\nconsumer: {target} = add(node: \"consumer\", operand_0: 0f32)\nproducer: @consumer.operand_0 = add(node: \"producer\", operand_0: 1f32)",
    ] {
        let mut raw = synthetic::json();
        raw["text"] = text.into();
        assert!(Workload::parse(&raw.to_string()).is_ok());
    }
}
