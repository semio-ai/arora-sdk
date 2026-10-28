//! Replay and statistics shared by native and WebAssembly.
use crate::{
    clock::Clock,
    forms::build::signatures,
    forms::{Form, FORMS},
    host::Bridge,
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
}

fn replay_once(workload: &Workload, form: &dyn Form, record: bool, clock: &dyn Clock) -> Replay {
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
    let mut bridge = Bridge::new(signatures(workload));
    let nodes = form.build(workload);
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    let tree = form
        .lower(nodes, &resolver, &bridge.index(), &mut bridge)
        .unwrap();
    let mut result = Replay {
        times: Vec::new(),
        calls: Vec::new(),
        dispatches: Vec::new(),
        written: Vec::new(),
        repetitions: 1,
        replay_times: Vec::new(),
    };
    for (tick, events) in workload.events.iter().enumerate() {
        let (calls, dispatches) = (bridge.module_calls, bridge.dispatches);
        let start = clock.now();
        let time = (workload.clock, Value::F64((tick + 1) as f64 * workload.dt));
        for (k, value) in [time].iter().chain(events) {
            // The host writes a key only when its value changes; a key the
            // tree also writes is compared with the store, not the last write.
            let changed = if workload.tree_keys.contains(k) {
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
        result.times.push(clock.now() - start);
        result.calls.push(bridge.module_calls - calls);
        result.dispatches.push(bridge.dispatches - dispatches);
        if record {
            result.written.push(values_hash(
                &workload
                    .compared
                    .iter()
                    .map(|&k| slots[k].get())
                    .collect::<Vec<_>>(),
            ));
        }
    }
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
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub calls: f64,
    pub dispatches: f64,
    pub phases: Vec<f64>,
    pub bins: Vec<(f64, f64)>,
}

pub fn summarize(workload: &Workload, runs: &[Replay]) -> Summary {
    let ticks = workload.events.len();
    let warm_up = workload.warm_up;
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
        p50: quantile(&sorted, 0.5),
        p95: quantile(&sorted, 0.95),
        p99: quantile(&sorted, 0.99),
        max: sorted[sorted.len() - 1],
        calls: mean(measured.clone().map(|t| runs[0].calls[t] as f64)),
        dispatches: mean(measured.clone().map(|t| runs[0].dispatches[t] as f64)),
        phases: (0..workload.phase_names.len())
            .map(|phase| {
                let ticks = measured.clone().filter(|&t| workload.phases[t] == phase);
                mean(ticks.map(|t| per_tick[t]))
            })
            .collect(),
        bins: (warm_up..ticks)
            .step_by(BIN)
            .map(|start| {
                let bin = start..(start + BIN).min(ticks);
                (
                    mean(bin.clone().map(|t| per_tick[t])),
                    mean(bin.map(|t| runs[0].calls[t] as f64)),
                )
            })
            .collect(),
    }
}

/// The first replay records agreement outside each tick's measured interval.
/// Additional replays contribute timing only, on freshly lowered trees.
pub fn replay(workload: &Workload, form: &dyn Form, record: bool, clock: &dyn Clock) -> Replay {
    replay_adaptive(workload, form, record, clock, 0.0)
}

/// Lengthen a sample by fresh replays until its timed ticks span the clock
/// budget. Each logical tick retains the mean across these repetitions.
/// Lowering, digesting and summary construction do not count toward the budget.
pub fn replay_adaptive(
    workload: &Workload,
    form: &dyn Form,
    record: bool,
    clock: &dyn Clock,
    minimum_us: f64,
) -> Replay {
    let mut result = replay_once(workload, form, record, clock);
    let mut elapsed: f64 = result.times[workload.warm_up..].iter().sum();
    if elapsed < minimum_us {
        result.replay_times.push(result.times.clone());
    }
    while elapsed < minimum_us {
        let next = replay_once(workload, form, false, clock);
        elapsed += next.times[workload.warm_up..].iter().sum::<f64>();
        result.repetitions += 1;
        for (sum, time) in result.times.iter_mut().zip(&next.times) {
            *sum += time;
        }
        result.replay_times.push(next.times);
    }
    for time in &mut result.times {
        *time /= result.repetitions as f64;
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
        .find(|form| form.name() == "detached")
        .expect("registered detached reference");
    let mut runs: Vec<Vec<Replay>> = forms.iter().map(|_| Vec::new()).collect();
    for sample in 0..samples {
        for (i, form) in forms.iter().enumerate() {
            runs[i].push(replay(workload, *form, sample == 0, clock));
        }
    }
    let reference = if let Some(i) = forms.iter().position(|form| form.name() == "detached") {
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

pub fn differences(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b).filter(|(a, b)| a != b).count() + a.len().abs_diff(b.len())
}

/// Stable FNV-1a over tagged values. Signed zero is canonicalized.
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
            Some(Value::F64(x)) => {
                eat(&[1]);
                eat(&(if *x == 0.0 {
                    0.0f64
                } else if x.is_nan() {
                    f64::NAN
                } else {
                    *x
                })
                .to_bits()
                .to_le_bytes());
            }
            Some(Value::Boolean(b)) => eat(&[2, u8::from(*b)]),
            Some(other) => {
                eat(&[3]);
                eat(format!("{other:?}").as_bytes());
            }
        }
    }
    format!("{hash:016x}")
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
                    "repetitions": m.runs.iter().map(|r| r.repetitions).collect::<Vec<_>>(),
                    "replayTimes": m.runs.iter().map(|r| &r.replay_times).collect::<Vec<_>>(),
                    "calls": m.runs[0].calls, "dispatches": m.runs[0].dispatches,
                    "written": m.runs[0].written, "differing": m.differing,
                }),
            )
        })
        .collect();
    serde_json::json!({"schema": 1, "platform": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS), "warmUp": workload.warm_up, "ticks": workload.events.len(), "dt": workload.dt, "phases": workload.phase_names, "forms": forms})
}
