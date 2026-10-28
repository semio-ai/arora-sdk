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
fn parser_rejects_a_second_writer_of_a_graph_calls_key() {
    let mut raw = synthetic::json();
    let error = |raw: &serde_json::Value| Workload::parse(&raw.to_string()).err().unwrap();
    // Two graph calls writing one key.
    raw["text"] = "running\n\na: {target} = add(node: \"a\", operand_0: 1f32)\nb: {target} = add(node: \"b\", operand_0: 2f32)".into();
    assert!(
        error(&raw).contains("no other writer: target"),
        "{}",
        error(&raw)
    );
    // A tree output on a key a graph call writes.
    raw["text"] = "parallel\n  unpack(values: f32[5], out_0: {target})\n  running\n\na: {target} = add(node: \"a\", operand_0: 1f32)".into();
    assert!(
        error(&raw).contains("no other writer: target"),
        "{}",
        error(&raw)
    );
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

// The store must win over the host's last write when another writer intervenes.
fn write_raw(sink: bool) -> serde_json::Value {
    let text = if sink {
        "running\n\nobserved: {observed} = add(operand_0: {out})\nwriter: {out} = add(operand_0: 2f64)"
    } else {
        "fallback\n  sequence\n    equal(a: {gate}, b: true)\n    unpack(values: f64[2], out_0: {out})\n  running"
    };
    serde_json::json!({"keys":[{"name":"clock","initial":0.0},{"name":"out","initial":0.0},{"name":"gate","initial":true},{"name":"observed","initial":0.0}],
        "text":text,"ticks":3,"warmUp":0,"clock":"clock","dt":1.0/60.0,
        "events":[[0,"out",1.0],[1,"gate",false],[2,"out",1.0]],"phases":{"names":["one"],"runs":[]}})
}
#[test]
fn host_reapplies_events_after_tree_writes() {
    assert_host_write(false);
}
#[test]
fn host_reapplies_events_after_sink_writes() {
    assert_host_write(true);
}
fn assert_host_write(sink: bool) {
    use arora_types::value::Value;
    let workload = Workload::parse(&write_raw(sink).to_string()).unwrap();
    for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
        let expected: Vec<_> = (if sink {
            vec![vec![2.0, 1.0], vec![2.0, 2.0], vec![2.0, 1.0]]
        } else {
            vec![vec![2.0], vec![2.0], vec![1.0]]
        })
        .into_iter()
        .map(|values| {
            measure::values_hash(
                &values
                    .into_iter()
                    .map(|x| Some(Value::F64(x)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
        assert_eq!(row.runs[0].written, expected, "{}", row.form);
    }
}
#[test]
fn tree_written_values_change_the_digest() {
    let mut raw = write_raw(false);
    let workload = Workload::parse(&raw.to_string()).unwrap();
    let first = measure::replay(&workload, FORMS[0], true, &Monotonic::default());
    raw["text"] = raw["text"]
        .as_str()
        .unwrap()
        .replace("f64[2]", "f64[3]")
        .into();
    let workload = Workload::parse(&raw.to_string()).unwrap();
    let second = measure::replay(&workload, FORMS[0], true, &Monotonic::default());
    assert_ne!(first.written[0], second.written[0]);
}

#[test]
fn timed_actions_progress_at_thirty_hz() {
    use arora_types::value::Value;
    for text in [
        "sequence\n  hold(t: {clock}, duration: 0.05, state: ())\n  unpack(values: f64[2], out_0: {out})",
        "unpack(values: @frame, out_0: {out})\n\nlate: greaterthan(lhs: {clock}, rhs: 0.09f32)\nframe: if(cond: @late, then: f64[2], else: f64[1])",
    ] {
        let mut raw = write_raw(false);
        raw["text"] = text.into(); raw["dt"] = (1.0/30.0).into(); raw["events"] = serde_json::json!([]);
        let workload = Workload::parse(&raw.to_string()).unwrap();
        for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
            assert_eq!(row.runs[0].written[2],measure::values_hash(&[Some(Value::F64(2.0))]));
            assert_eq!(row.differing,0);
        }
    }
}
/// Native frame selection leaves only the frame due at the device time.
#[test]
fn a_tree_call_leaves_its_last_write_to_an_output() {
    use arora_types::value::Value;
    let mut raw = write_raw(false);
    raw["text"] = "unpack(values: @frame, out_0: {out})\n\ndue: greaterthan(lhs: {clock}, rhs: 0f32)\nframe: if(cond: @due, then: f64[0], else: f64[1])".into();
    raw["events"] = serde_json::json!([]);
    let workload = Workload::parse(&raw.to_string()).unwrap();
    for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
        assert_eq!(
            row.runs[0].written[0],
            measure::values_hash(&[Some(Value::F64(0.0))])
        );
    }
}

/// By hand: each tick has one hold and one say. The detached constant runs
/// once; the two native unpacks demand its same value. The nested parallel
/// has six indirect ticks (root, hold, inner parallel, two unpacks, say).
/// Attached adds its wrapper and its graph leaf, and runs the constant each tick.
#[test]
fn counting_includes_reentrant_dataflow_host_and_real_calls_once() {
    let mut raw = write_raw(false);
    raw["text"] = "parallel\n  hold(t: {clock}, duration: 0, state: ())\n  parallel\n    unpack(values: @values, out_0: {out})\n    unpack(values: @values, out_0: {observed})\n  say(text: \"\", voice: \"\", viseme: {viseme})\n\nvalues: constant(param.value: f64[3])".into();
    raw["keys"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"name":"viseme","initial":""}));
    raw["events"] = serde_json::json!([]);
    let workload = Workload::parse(&raw.to_string()).unwrap();
    for form in FORMS {
        let run = measure::replay(&workload, *form, true, &Monotonic::default());
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
    let mut raw = write_raw(false);
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
    let mut raw = write_raw(false);
    raw["text"] = "{introduced} = add(operand_0: @smooth.state[.0])\n\nsmooth: damp(in: f32[1,2], dt: 0.1f32, param.half_life: 0.1f32, state: ())".into();
    raw["events"] = serde_json::json!([]);
    let workload = Workload::parse(&raw.to_string()).unwrap();
    assert_eq!(workload.keys.last().unwrap().0, "introduced");
    let rows = measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap();
    assert!(rows.iter().all(|r| r.differing == 0));
    let expected = measure::values_hash(&[Some(arora_types::value::Value::F32(1.0))]);
    for row in rows {
        assert_eq!(row.runs[0].written, vec![expected.clone(); 3]);
    }
}

/// Scatter's runtime key resolver writes names outside the envelope. Its
/// writes enter the digest, and an event must restore a key it overwrote.
#[test]
fn scatter_writes_dynamic_keys_and_host_events_remain_authoritative() {
    use arora_types::value::Value;
    let mut raw = write_raw(false);
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
