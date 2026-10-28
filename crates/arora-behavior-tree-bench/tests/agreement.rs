use arora_behavior_tree_bench::{
    clock::Monotonic,
    forms::{self, FORMS},
    measure,
    model::Workload,
    scale::Scenario,
    synthetic,
};

#[test]
fn every_registered_form_agrees_and_meets_its_count_contract() {
    let fixtures = std::iter::once(synthetic::workload())
        .chain(Scenario::ALL.map(|s| s.workload(10, 16, 144)));
    for (fixture, workload) in fixtures.enumerate() {
        let measurements = measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap();
        for (form, m) in FORMS.iter().zip(measurements) {
            assert_eq!(
                m.differing,
                0,
                "fixture {fixture}: {} writes different values",
                form.name()
            );
            let counts = (
                m.runs[0].calls.iter().sum(),
                m.runs[0].dispatches.iter().sum(),
            );
            assert_eq!(
                counts,
                form.fixture_counts()[fixture],
                "fixture {fixture}: {} count contract",
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
fn parser_rejects_invalid_dimensions_references_and_signatures() {
    let mut raw = synthetic::json();
    raw["warmUp"] = raw["ticks"].clone();
    assert!(Workload::parse(&raw.to_string()).is_err());
    let mut raw = synthetic::json();
    raw["calls"][0]["args"][0][1] = serde_json::json!({"node": 0});
    assert!(Workload::parse(&raw.to_string()).is_err());
    let mut raw = synthetic::json();
    raw["calls"][0]["op"] = "unknown".into();
    assert!(Workload::parse(&raw.to_string()).is_err());
    let mut raw = synthetic::json();
    raw["tree"] =
        serde_json::json!({"k": "call", "op": "pulse", "args": [], "outputs": [], "state": 0});
    assert!(Workload::parse(&raw.to_string()).is_err());
    let mut raw = synthetic::json();
    raw["events"][0][1] = 9000.into();
    assert!(Workload::parse(&raw.to_string()).is_err());
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
fn write_fixture(sink: bool) -> Workload {
    let tree = if sink {
        serde_json::json!({"k":"run"})
    } else {
        serde_json::json!({"k":"fallback", "children":[
            {"k":"seq", "children":[{"k":"equal", "key":2, "value":true},
                {"k":"call", "op":"set", "args":[["v", {"value":2.0}]], "outputs":[["v",1]], "state":0}]},
            {"k":"run"}]})
    };
    Workload::parse(&serde_json::json!({
        "keys":[{"name":"clock","initial":0.0},{"name":"out","initial":0.0},{"name":"gate","initial":true},{"name":"observed","initial":0.0}],
        "calls": if sink { serde_json::json!([{"op":"first", "args":[["x",{"key":1}]], "publishes":3},{"op":"first", "args":[["x",{"value":2.0}]], "publishes":1}]) } else { serde_json::json!([]) },
        "tree":tree, "ticks":3,"warmUp":0,"clock":0,"dt":1.0/60.0,
        "events":[[0,1,1.0],[1,2,false],[2,1,1.0]],
        "phases":{"names":["one"],"runs":[]}
    }).to_string()).unwrap()
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
    let workload = write_fixture(sink);
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
    let mut workload = write_fixture(false);
    let first = measure::replay(&workload, FORMS[0], true, &Monotonic::default());
    if let arora_behavior_tree_bench::model::Tree::Composite(_, children) = &mut workload.tree {
        if let arora_behavior_tree_bench::model::Tree::Composite(_, seq) = &mut children[0] {
            if let arora_behavior_tree_bench::model::Tree::Call(_, _, args, _, _) = &mut seq[1] {
                args[0] = arora_behavior_tree_bench::model::Source::Value(
                    arora_types::value::Value::F64(3.0),
                );
            }
        }
    }
    let second = measure::replay(&workload, FORMS[0], true, &Monotonic::default());
    assert_ne!(first.written[0], second.written[0]);
}

#[test]
fn timed_actions_progress_at_thirty_hz() {
    use arora_types::value::Value;
    let mut raw = serde_json::json!({
        "keys":[{"name":"clock","initial":0.0},{"name":"out","initial":0.0}],
        "calls":[],"ticks":5,"warmUp":0,"clock":0,"dt":1.0/30.0,"events":[],
        "phases":{"names":["one"],"runs":[]},
        "tree":{"k":"seq","children":[
            {"k":"call","op":"hold","args":[["t",{"key":0}],["duration",{"value":0.05}]],"outputs":[],"state":2},
            {"k":"call","op":"set","args":[["v",{"value":2.0}]],"outputs":[["v",1]],"state":0}]}
    });
    for keyframes in [false, true] {
        if keyframes {
            raw["tree"] = serde_json::json!({"k":"call","op":"keyframes","args":[["t",{"key":0}],["n",{"value":1.0}],["offset",{"value":0.0}],["v",{"value":1.0}],["offset",{"value":0.05}],["v",{"value":2.0}]],"outputs":[["v",1]],"state":3});
        }
        let workload = Workload::parse(&raw.to_string()).unwrap();
        for row in measure::measure(&workload, FORMS, 1, &Monotonic::default()).unwrap() {
            assert_eq!(
                row.runs[0].written[2],
                measure::values_hash(&[Some(Value::F64(2.0))])
            );
            assert_eq!(row.differing, 0);
        }
    }
}
