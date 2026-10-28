use arora_behavior_tree_bench::{
    clock::{Clock, Monotonic},
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
    };
    let runs = vec![
        replay(vec![100.0, 1.0, 2.0, 3.0]),
        replay(vec![100.0, 20.0, 1.0, 5.0]),
        replay(vec![100.0, 3.0, 4.0, 9.0]),
    ];
    let s = measure::summarize(&workload, &runs);
    assert_eq!(s.mean, 10.0 / 3.0);
    assert_eq!(s.p50, 3.0);
    assert!((s.p95 - 4.8).abs() < 1e-12);
    assert_eq!(s.calls, 2.0);
    assert_eq!(s.dispatches, 5.0);
    assert_eq!(s.phases, vec![3.0, 3.5]);
}

#[test]
fn clock_budget_lengthens_samples_without_changing_counts_or_digests() {
    use std::cell::Cell;
    struct TestClock(Cell<f64>);
    impl Clock for TestClock {
        fn now(&self) -> f64 {
            let n = self.0.get();
            self.0.set(n + 1.0);
            n
        }
    }
    let clock = TestClock(Cell::new(0.0));
    let workload = synthetic::workload();
    let run = measure::replay_adaptive(&workload, FORMS[0], true, &clock, 400.0);
    assert_eq!(run.repetitions, 3);
    assert_eq!(run.times, vec![1.0; synthetic::TICKS]);
    assert_eq!(
        run.calls.iter().sum::<u64>(),
        FORMS[0].fixture_counts()[0].0
    );
    assert_eq!(run.written.len(), synthetic::TICKS);
    assert_eq!(run.replay_times.len(), 3);
}
