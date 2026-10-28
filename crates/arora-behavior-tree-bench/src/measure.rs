//! Replay and statistics shared by native and WebAssembly.
use crate::{
    clock::Clock,
    forms::{Form, FORMS, REFERENCE},
    host::{Bridge, Counting},
    model::Workload,
};
use arora_simple_data_store::SimpleDataStore;
use arora_types::{
    data::{DataStore, Key},
    value::Value,
};
use serde::Serialize;
use std::hint::black_box;

pub const BIN: usize = 15;
#[derive(Serialize)]
pub struct Replay {
    pub times: Vec<f64>,
    pub calls: Vec<u64>,
    pub dispatches: Vec<u64>,
    /// One FNV-1a digest of all compared keys per tick, warm-up included.
    pub written: Vec<String>,
    pub repetitions: usize,
    pub replay_times: Vec<Vec<f64>>,
    pub timing_mode: TimingMode,
    pub replay_us: Vec<f64>,
    pub mean_us: f64,
    pub step_us: f64,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimingMode {
    Tick,
    Replay,
}

fn replay_once(
    workload: &Workload,
    form: &dyn Form,
    record: bool,
    clock: &dyn Clock,
    mode: TimingMode,
) -> Replay {
    with_fresh_dataflow(|dataflow| replay_on(workload, form, record, clock, mode, dataflow))
}

fn with_fresh_dataflow<T>(
    run: impl FnOnce(&mut arora_dataflow_nodes::functions::DataflowNodes) -> T,
) -> T {
    run(&mut arora_dataflow_nodes::functions::DataflowNodes::new())
}

fn replay_on(
    workload: &Workload,
    form: &dyn Form,
    record: bool,
    clock: &dyn Clock,
    mode: TimingMode,
    dataflow: &mut arora_dataflow_nodes::functions::DataflowNodes,
) -> Replay {
    let _device = crate::modules::fresh_device();
    let store = SimpleDataStore::new();
    let slots: Vec<_> = workload
        .keys
        .iter()
        .map(|(name, initial)| {
            let slot = store.slot(&Key::from(name.as_str()));
            if let Some(value) = initial {
                slot.set(Some(value.clone())).unwrap();
            }
            slot
        })
        .collect();
    let mut written: Vec<_> = workload
        .keys
        .iter()
        .map(|(_, initial)| initial.clone())
        .collect();
    let mut host = Bridge::new(workload.dt);
    let counts = host.counts.clone();
    let mut dataflow_bridge = dataflow.bridge(&mut host);
    let mut bridge = Counting {
        inner: &mut dataflow_bridge,
        counts: counts.clone(),
    };
    let nodes = form.build(workload);
    let resolving_store = store.clone();
    let resolver: std::rc::Rc<arora_behavior_tree::variable::VariableResolver<'static>> =
        std::rc::Rc::new(move |name: &str| Some(resolving_store.slot(&Key::from(name))));
    let tree = form
        .lower(nodes, &resolver, Bridge::index(), &mut bridge)
        .unwrap();
    // Every per-tick vector holds the whole replay before timing starts, so
    // a replay-wide interval never includes a reallocation.
    let ticks = workload.events.len();
    let mut result = Replay {
        times: Vec::with_capacity(ticks),
        calls: Vec::with_capacity(ticks),
        dispatches: Vec::with_capacity(ticks),
        written: Vec::with_capacity(if record { ticks } else { 0 }),
        repetitions: 1,
        replay_times: Vec::new(),
        timing_mode: mode,
        replay_us: Vec::new(),
        mean_us: 0.0,
        step_us: clock.step_us(),
    };
    let mut interval_start = 0.0;
    for (tick, events) in workload.events.iter().enumerate() {
        let (calls, dispatches) = (counts.module_calls.get(), counts.dispatches.get());
        if mode == TimingMode::Replay && tick == workload.warm_up {
            interval_start = clock.now();
        }
        let start = if mode == TimingMode::Tick {
            clock.now()
        } else {
            0.0
        };
        let time = (workload.clock, Value::F64((tick + 1) as f64 * workload.dt));
        for (k, value) in [time].iter().chain(events) {
            // The host writes a key only when its value changes; a key the
            // tree or a graph sink also writes is compared with the store.
            let changed = if workload.writer_keys.contains(k) {
                slots[*k].get().as_ref() != Some(value)
            } else {
                written[*k].as_ref() != Some(value)
            };
            if changed {
                slots[*k].set(Some(value.clone())).unwrap();
                written[*k] = Some(value.clone());
            }
        }
        black_box(tree.tick(&mut bridge).unwrap());
        #[cfg(any(test, feature = "bench-check"))]
        clock.tick_finished();
        if mode == TimingMode::Tick {
            result.times.push(clock.now() - start);
        }
        if mode == TimingMode::Replay && tick + 1 == workload.events.len() {
            result.replay_us.push(clock.now() - interval_start);
        }
        result.calls.push(counts.module_calls.get() - calls);
        result.dispatches.push(counts.dispatches.get() - dispatches);
        if record {
            if workload.dynamic_keys {
                let snapshot = store.snapshot();
                let mut entries: Vec<_> = snapshot.storage.into_iter().collect();
                entries.sort_by(|a, b| a.0.get_path().cmp(b.0.get_path()));
                let values = entries
                    .into_iter()
                    .flat_map(|(key, value)| [Some(Value::String(key.get_path().into())), value])
                    .collect::<Vec<_>>();
                result.written.push(values_hash(&values));
            } else {
                result.written.push(values_hash(
                    &workload
                        .compared
                        .iter()
                        .map(|&k| slots[k].get())
                        .collect::<Vec<_>>(),
                ));
            }
        }
    }
    result.mean_us = if mode == TimingMode::Tick {
        mean(result.times[workload.warm_up..].iter().copied())
    } else {
        result.replay_us[0] / (workload.events.len() - workload.warm_up) as f64
    };
    tree.unregister(&mut bridge);
    result
}

/// Quantile: linear between the two nearest order statistics.
pub fn quantile(sorted: &[f64], p: f64) -> f64 {
    let at = (sorted.len() - 1) as f64 * p;
    let (low, high) = (at.floor() as usize, at.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (at - low as f64)
}

pub fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = values.fold((0.0, 0), |(sum, count), x| (sum + x, count + 1));
    sum / count as f64
}

/// One form's statistics over its replays: per tick, the median time over
/// the replays; then, over the measured ticks, the mean, quantiles and
/// maximum time, the mean calls and dispatches of the first replay, the mean
/// time in each phase, and 15-tick bins of time and calls.
#[derive(Serialize)]
pub struct Summary {
    pub mean: f64,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub max: Option<f64>,
    pub calls: f64,
    pub dispatches: f64,
    pub phases: Vec<Option<f64>>,
    pub bins: Vec<(Option<f64>, f64)>,
}

pub fn summarize(workload: &Workload, runs: &[Replay]) -> Summary {
    for run in runs.iter().skip(1) {
        assert_counts(&runs[0], run);
    }
    let ticks = workload.events.len();
    let warm_up = workload.warm_up;
    if runs.iter().any(|r| r.timing_mode == TimingMode::Replay) {
        return Summary {
            mean: mean(runs.iter().map(|r| r.mean_us)),
            p50: None,
            p95: None,
            p99: None,
            max: None,
            calls: mean(runs[0].calls[warm_up..].iter().map(|&x| x as f64)),
            dispatches: mean(runs[0].dispatches[warm_up..].iter().map(|&x| x as f64)),
            phases: vec![None; workload.phase_names.len()],
            bins: (warm_up..ticks)
                .step_by(BIN)
                .map(|start| {
                    (
                        None,
                        mean(
                            runs[0].calls[start..(start + BIN).min(ticks)]
                                .iter()
                                .map(|&x| x as f64),
                        ),
                    )
                })
                .collect(),
        };
    }
    let per_tick: Vec<f64> = (0..ticks)
        .map(|t| {
            let mut times: Vec<_> = runs.iter().map(|run| run.times[t]).collect();
            times.sort_by(f64::total_cmp);
            quantile(&times, 0.5)
        })
        .collect();
    let measured = warm_up..ticks;
    let mut sorted = per_tick[measured.clone()].to_vec();
    sorted.sort_by(f64::total_cmp);
    Summary {
        mean: mean(sorted.iter().copied()),
        p50: Some(quantile(&sorted, 0.5)),
        p95: Some(quantile(&sorted, 0.95)),
        p99: Some(quantile(&sorted, 0.99)),
        max: Some(sorted[sorted.len() - 1]),
        calls: mean(measured.clone().map(|t| runs[0].calls[t] as f64)),
        dispatches: mean(measured.clone().map(|t| runs[0].dispatches[t] as f64)),
        phases: (0..workload.phase_names.len())
            .map(|phase| {
                let ticks = measured.clone().filter(|&t| workload.phases[t] == phase);
                let value = mean(ticks.map(|t| per_tick[t]));
                value.is_finite().then_some(value)
            })
            .collect(),
        bins: (warm_up..ticks)
            .step_by(BIN)
            .map(|start| {
                let bin = start..(start + BIN).min(ticks);
                (
                    Some(mean(bin.clone().map(|t| per_tick[t]))),
                    mean(bin.map(|t| runs[0].calls[t] as f64)),
                )
            })
            .collect(),
    }
}

/// Record agreement outside measured intervals when requested. Additional
/// samples contribute timing only, on freshly lowered trees.
pub fn replay(workload: &Workload, form: &dyn Form, record: bool, clock: &dyn Clock) -> Replay {
    replay_adaptive(workload, form, record, clock, 0.0)
}

/// A pilot fixes the timing mode and replay count before collecting a sample.
/// Coarse clocks time all measured ticks as one interval; agreement is recorded
/// in a separate replay so digest work never enters that interval.
pub fn replay_adaptive(
    workload: &Workload,
    form: &dyn Form,
    record: bool,
    clock: &dyn Clock,
    minimum_us: f64,
) -> Replay {
    let measured = workload.events.len() - workload.warm_up;
    // Discarding real work before sampling also warms each process's code,
    // allocator and CPU state; startup transients otherwise dominate first tests.
    let pilot = replay_once(workload, form, false, clock, TimingMode::Replay);
    let elapsed = pilot.replay_us[0];
    let mode = if crate::clock::coarse(clock.step_us(), elapsed, measured) {
        TimingMode::Replay
    } else {
        TimingMode::Tick
    };
    // At least one clock step prevents division by zero on an unresolved pilot.
    let repetitions = (minimum_us / elapsed.max(clock.step_us()).max(f64::MIN_POSITIVE))
        .ceil()
        .max(1.0) as usize;
    let mut result = replay_once(
        workload,
        form,
        record && mode == TimingMode::Tick,
        clock,
        mode,
    );
    assert_counts(&pilot, &result);
    if record && mode == TimingMode::Replay {
        struct Untimed;
        impl Clock for Untimed {
            fn now(&self) -> f64 {
                0.0
            }
        }
        let recorded = replay_once(workload, form, true, &Untimed, TimingMode::Tick);
        assert_counts(&result, &recorded);
        result.written = recorded.written;
    }
    if mode == TimingMode::Tick {
        result.replay_times.push(result.times.clone());
    }
    for _ in 1..repetitions {
        let next = replay_once(workload, form, false, clock, mode);
        assert_eq!(
            result.calls, next.calls,
            "calls must be identical across replays"
        );
        assert_eq!(
            result.dispatches, next.dispatches,
            "dispatches must be identical across replays"
        );
        result.mean_us += next.mean_us;
        for (sum, time) in result.times.iter_mut().zip(&next.times) {
            *sum += time;
        }
        if mode == TimingMode::Tick {
            result.replay_times.push(next.times);
        }
        result.replay_us.extend(next.replay_us);
    }
    result.repetitions = repetitions;
    result.mean_us /= repetitions as f64;
    for time in &mut result.times {
        *time /= repetitions as f64;
    }
    result
}

#[derive(Serialize)]
pub struct Measurement {
    pub form: String,
    pub summary: Summary,
    pub runs: Vec<Replay>,
    pub differing: usize,
}

/// Interleave forms within each sample; always obtain detached's reference,
/// even when the user selects a single other form for timing.
pub fn measure(
    workload: &Workload,
    forms: &[&dyn Form],
    samples: usize,
    clock: &dyn Clock,
) -> Result<Vec<Measurement>, String> {
    if samples == 0 || samples.is_multiple_of(2) || forms.is_empty() {
        return Err("positive odd samples and at least one form required".into());
    }
    let reference_form = FORMS
        .iter()
        .find(|form| form.name() == REFERENCE)
        .expect("registered detached reference");
    let mut runs: Vec<Vec<Replay>> = forms.iter().map(|_| Vec::new()).collect();
    for sample in 0..samples {
        for (i, form) in forms.iter().enumerate() {
            let run = replay(workload, *form, sample == 0, clock);
            if let Some(first) = runs[i].first() {
                assert_counts(first, &run);
            }
            runs[i].push(run);
        }
    }
    let reference = if let Some(i) = forms.iter().position(|form| form.name() == REFERENCE) {
        runs[i][0].written.clone()
    } else {
        replay(workload, *reference_form, true, clock).written
    };
    Ok(forms
        .iter()
        .zip(runs)
        .map(|(form, runs)| {
            let differing = differences(&reference, &runs[0].written);
            Measurement {
                form: form.name().into(),
                summary: summarize(workload, &runs),
                runs,
                differing,
            }
        })
        .collect())
}

pub fn assert_counts(first: &Replay, next: &Replay) {
    assert_eq!(
        first.calls, next.calls,
        "calls must be identical across samples"
    );
    assert_eq!(
        first.dispatches, next.dispatches,
        "dispatches must be identical across samples"
    );
}

pub fn differences(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b).filter(|(a, b)| a != b).count() + a.len().abs_diff(b.len())
}

/// Stable FNV-1a over tagged values: equal values hash alike. Every float a
/// value holds has its signed zero and NaN canonicalized, and a record's
/// fields are taken in name order, whatever order its map keeps them in.
/// Digests are comparable only within one platform's math implementation.
pub fn values_hash(values: &[Option<Value>]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for value in values {
        match value {
            None => eat(&[0]),
            Some(value) => hash_value(value, &mut eat),
        }
    }
    format!("{hash:016x}")
}

fn f32_bits(x: f32) -> [u8; 4] {
    let x = if x == 0.0 {
        0.0
    } else if x.is_nan() {
        f32::NAN
    } else {
        x
    };
    x.to_bits().to_le_bytes()
}
fn f64_bits(x: f64) -> [u8; 8] {
    let x = if x == 0.0 {
        0.0
    } else if x.is_nan() {
        f64::NAN
    } else {
        x
    };
    x.to_bits().to_le_bytes()
}

fn hash_value(value: &Value, eat: &mut dyn FnMut(&[u8])) {
    let len = |eat: &mut dyn FnMut(&[u8]), n: usize| eat(&(n as u64).to_le_bytes());
    let text = |eat: &mut dyn FnMut(&[u8]), s: &str| {
        eat(&(s.len() as u64).to_le_bytes());
        eat(s.as_bytes());
    };
    let fields = |eat: &mut dyn FnMut(&[u8]), fields: &[arora_types::value::StructureField]| {
        eat(&(fields.len() as u64).to_le_bytes());
        for field in fields {
            eat(field.id.as_bytes());
            hash_value(&field.value, eat);
        }
    };
    macro_rules! array {
        ($tag:expr, $items:expr, $bytes:expr) => {{
            eat(&[$tag]);
            len(eat, $items.len());
            for item in $items {
                eat(&$bytes(*item));
            }
        }};
    }
    match value {
        Value::Unit => eat(&[6]),
        Value::Boolean(b) => eat(&[2, u8::from(*b)]),
        Value::U8(x) => eat(&[7, *x]),
        Value::U16(x) => {
            eat(&[8]);
            eat(&x.to_le_bytes());
        }
        Value::U32(x) => {
            eat(&[9]);
            eat(&x.to_le_bytes());
        }
        Value::U64(x) => {
            eat(&[10]);
            eat(&x.to_le_bytes());
        }
        Value::I8(x) => eat(&[11, *x as u8]),
        Value::I16(x) => {
            eat(&[12]);
            eat(&x.to_le_bytes());
        }
        Value::I32(x) => {
            eat(&[13]);
            eat(&x.to_le_bytes());
        }
        Value::I64(x) => {
            eat(&[14]);
            eat(&x.to_le_bytes());
        }
        Value::F32(x) => {
            eat(&[5]);
            eat(&f32_bits(*x));
        }
        Value::F64(x) => {
            eat(&[1]);
            eat(&f64_bits(*x));
        }
        Value::String(s) => {
            eat(&[3]);
            text(eat, s);
        }
        Value::Option(inner) => match inner {
            None => eat(&[15, 0]),
            Some(inner) => {
                eat(&[15, 1]);
                hash_value(inner, eat);
            }
        },
        Value::Structure(structure) => {
            eat(&[16]);
            eat(structure.id.as_bytes());
            fields(eat, &structure.fields);
        }
        Value::Enumeration(enumeration) => {
            eat(&[17]);
            eat(enumeration.id.as_bytes());
            eat(enumeration.variant_id.as_bytes());
            hash_value(&enumeration.value, eat);
        }
        Value::ArrayBoolean(items) => array!(18, items, |b: bool| [u8::from(b)]),
        Value::ArrayU8(items) => array!(19, items, |x: u8| [x]),
        Value::ArrayU16(items) => array!(20, items, u16::to_le_bytes),
        Value::ArrayU32(items) => array!(21, items, u32::to_le_bytes),
        Value::ArrayU64(items) => array!(22, items, u64::to_le_bytes),
        Value::ArrayI8(items) => array!(23, items, i8::to_le_bytes),
        Value::ArrayI16(items) => array!(24, items, i16::to_le_bytes),
        Value::ArrayI32(items) => array!(25, items, i32::to_le_bytes),
        Value::ArrayI64(items) => array!(26, items, i64::to_le_bytes),
        Value::ArrayF32(items) => array!(27, items, f32_bits),
        Value::ArrayF64(items) => array!(28, items, f64_bits),
        Value::ArrayString(items) => {
            eat(&[29]);
            len(eat, items.len());
            for item in items {
                text(eat, item);
            }
        }
        Value::ArrayValue(items) => {
            eat(&[30]);
            len(eat, items.len());
            for item in items {
                hash_value(item, eat);
            }
        }
        Value::ArrayStructure { id, elements } => {
            eat(&[31]);
            eat(id.as_bytes());
            len(eat, elements.len());
            for element in elements {
                fields(eat, &element.fields);
            }
        }
        Value::ArrayEnumeration { id, elements } => {
            eat(&[32]);
            eat(id.as_bytes());
            len(eat, elements.len());
            for element in elements {
                eat(element.variant_id.as_bytes());
                hash_value(&element.value, eat);
            }
        }
        Value::KeyValue(record) => {
            eat(&[33]);
            eat(record.id.as_bytes());
            let mut names: Vec<_> = record.fields.iter().collect();
            names.sort_by(|a, b| a.0.cmp(b.0));
            len(eat, names.len());
            for (name, field) in names {
                text(eat, name);
                eat(field.id.as_bytes());
                text(eat, &field.name);
                match &field.value {
                    None => eat(&[0]),
                    Some(value) => {
                        eat(&[1]);
                        hash_value(value, eat);
                    }
                }
            }
        }
        Value::Uuid(id) => {
            eat(&[34]);
            eat(id.as_bytes());
        }
    }
}

/// One round's raw record, used by every front end and the comparison tools.
pub fn record(workload: &Workload, measurements: &[Measurement]) -> serde_json::Value {
    let forms: serde_json::Map<_, _> = measurements
        .iter()
        .map(|m| {
            (
                m.form.clone(),
                serde_json::json!({
                    "summary": m.summary,
                    "times": m.runs.iter().map(|r| &r.times).collect::<Vec<_>>(),
                    "timingMode": m.runs.iter().map(|r| r.timing_mode).collect::<Vec<_>>(),
                    "sampleMeans": m.runs.iter().map(|r| r.mean_us).collect::<Vec<_>>(),
                    "replayUs": m.runs.iter().map(|r| &r.replay_us).collect::<Vec<_>>(),
                    "repetitions": m.runs.iter().map(|r| r.repetitions).collect::<Vec<_>>(),
                    "replayTimes": m.runs.iter().map(|r| &r.replay_times).collect::<Vec<_>>(),
                    "calls": m.runs[0].calls, "dispatches": m.runs[0].dispatches,
                    "written": m.runs[0].written, "differing": m.differing,
                }),
            )
        })
        .collect();
    serde_json::json!({"schema": 1, "platform": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS), "warmUp": workload.warm_up, "ticks": workload.events.len(), "dt": workload.dt, "phases": workload.phase_names, "forms": forms, "clock": {"stepUs": measurements[0].runs[0].step_us, "modes": measurements.iter().map(|m| (m.form.clone(), serde_json::json!(m.runs.iter().map(|r| r.timing_mode).collect::<Vec<_>>()))).collect::<serde_json::Map<_, _>>()}})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replays prepare real per-node sites, but the next replay starts with
    /// none of their decoded parameters, layouts or retained runtime.
    #[test]
    fn replays_start_with_empty_dataflow_sites() {
        let workload = crate::synthetic::workload();
        for form in crate::forms::FORMS {
            let mut previous: Option<Replay> = None;
            for _ in 0..2 {
                let run = with_fresh_dataflow(|dataflow| {
                    assert_eq!(dataflow.sites(), 0, "a replay inherited prepared sites");
                    let run = replay_on(
                        &workload,
                        *form,
                        true,
                        &crate::clock::Monotonic::default(),
                        TimingMode::Tick,
                        dataflow,
                    );
                    assert!(
                        dataflow.sites() > 1,
                        "real nodes must have prepared their sites"
                    );
                    run
                });
                if let Some(previous) = previous {
                    assert_counts(&previous, &run);
                    assert_eq!(previous.written, run.written);
                }
                previous = Some(run);
            }
        }
    }
}
