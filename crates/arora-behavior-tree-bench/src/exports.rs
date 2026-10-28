//! C ABI for WASI preview 1. The README specifies indices and pointer lifetimes.
use crate::{
    clock::{Monotonic, WithStep},
    forms::{Form, FORMS, REFERENCE},
    measure::{self, Measurement, Replay},
    model::Workload,
    scale::{Scenario, SIZES},
    DEFAULT_TICKS, DEFAULT_WARM_UP,
};
use std::{
    cell::{Cell, RefCell},
    sync::OnceLock,
};

struct Session {
    workload: Workload,
    names: String,
    current: Option<usize>,
    runs: Vec<Vec<Replay>>,
    reference: Option<Vec<String>>,
    row: Vec<f64>,
    series: Vec<f64>,
}
thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static ROW: Cell<[f64; 7]> = const { Cell::new([0.0; 7]) };
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static STEP_US: Cell<f64> = const { Cell::new(0.0) };
    static MINIMUM_US: Cell<f64> = const { Cell::new(0.0) };
}
fn form_names() -> String {
    FORMS.iter().map(|f| f.name()).collect::<Vec<_>>().join(",")
}
fn names() -> &'static str {
    static NAMES: OnceLock<String> = OnceLock::new();
    NAMES.get_or_init(|| {
        format!(
            "{};{}",
            Scenario::ALL
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(","),
            form_names()
        )
    })
}
fn sample(workload: &Workload, form: &dyn Form, record: bool) -> Replay {
    measure::replay_adaptive(
        workload,
        form,
        record,
        &WithStep(Monotonic::default(), STEP_US.get()),
        MINIMUM_US.get(),
    )
}
fn result(workload: &Workload, measurements: &[Measurement]) {
    RESULT.set(serde_json::to_vec(&measure::record(workload, measurements)).unwrap());
}
fn hash(words: &[String]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for word in words {
        for b in word.as_bytes() {
            h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[no_mangle]
pub extern "C" fn bench_names() -> *const u8 {
    names().as_ptr()
}
#[no_mangle]
pub extern "C" fn bench_names_len() -> u32 {
    names().len() as u32
}
#[no_mangle]
pub extern "C" fn bench_result() -> *const u8 {
    RESULT.with_borrow(|r| r.as_ptr())
}
#[no_mangle]
pub extern "C" fn bench_result_len() -> u32 {
    RESULT.with_borrow(|r| r.len() as u32)
}
#[no_mangle]
pub extern "C" fn bench_minimum_sample_us(us: f64) -> u32 {
    if !us.is_finite() || us < 0.0 {
        return 1;
    }
    MINIMUM_US.set(us);
    0
}

#[no_mangle]
pub extern "C" fn bench_alloc(len: u32) -> *mut u8 {
    Box::into_raw(vec![0u8; len as usize].into_boxed_slice()).cast::<u8>()
}
/// # Safety
/// `ptr` and `len` must be a live allocation from `bench_alloc`, freed once.
#[no_mangle]
pub unsafe extern "C" fn bench_free(ptr: *mut u8, len: u32) {
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
        ptr,
        len as usize,
    )));
}

#[no_mangle]
pub extern "C" fn bench_workload_names() -> *const u8 {
    SESSION.with_borrow(|s| {
        s.as_ref().map_or_else(
            || {
                names()
                    .as_ptr()
                    .wrapping_add(names().find(';').unwrap() + 1)
            },
            |s| s.names.as_ptr(),
        )
    })
}
#[no_mangle]
pub extern "C" fn bench_workload_names_len() -> u32 {
    SESSION.with_borrow(|s| {
        s.as_ref()
            .map_or_else(|| form_names().len() as u32, |s| s.names.len() as u32)
    })
}
/// # Safety
/// `ptr` must point to `len` initialized bytes for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn bench_workload_load(ptr: *const u8, len: u32) -> u32 {
    SESSION.set(None);
    RESULT.set(Vec::new());
    let Ok(text) = std::str::from_utf8(std::slice::from_raw_parts(ptr, len as usize)) else {
        return 1;
    };
    let Ok(workload) = Workload::parse(text) else {
        return 1;
    };
    load(workload);
    0
}
fn load(workload: Workload) {
    let names = format!(
        "{};{};{},{},{},{}",
        form_names(),
        workload.phase_names.join(","),
        workload
            .nodes
            .0
            .iter()
            .filter(|n| !workload.attached.contains(&n.id))
            .count(),
        workload.events.len(),
        workload.warm_up,
        workload.dt
    );
    SESSION.set(Some(Session {
        workload,
        names,
        current: None,
        runs: FORMS.iter().map(|_| Vec::new()).collect(),
        reference: None,
        row: Vec::new(),
        series: Vec::new(),
    }));
}
#[no_mangle]
pub extern "C" fn bench_scale_load(n: u32, scenario: u32, warm_up: u32, ticks: u32) -> u32 {
    let Some(scenario) = Scenario::ALL.get(scenario as usize) else {
        return 1;
    };
    if n == 0 || ticks == 0 {
        return 1;
    }
    load(scenario.workload(n as usize, warm_up as usize, ticks as usize));
    0
}

#[no_mangle]
pub extern "C" fn bench_workload_begin(form: u32) -> u32 {
    SESSION.with_borrow_mut(|s| {
        let Some(s) = s else {
            return 1;
        };
        if form as usize >= FORMS.len() {
            return 1;
        }
        s.current = Some(form as usize);
        s.runs[form as usize].clear();
        s.row.clear();
        s.series.clear();
        0
    })
}
/// Select an already begun form to interleave samples without discarding runs.
#[no_mangle]
pub extern "C" fn bench_workload_select(form: u32) -> u32 {
    SESSION.with_borrow_mut(|s| {
        let Some(s) = s else {
            return 1;
        };
        if form as usize >= FORMS.len() {
            return 1;
        }
        s.current = Some(form as usize);
        0
    })
}
#[no_mangle]
pub extern "C" fn bench_workload_sample() -> u32 {
    SESSION.with_borrow_mut(|s| {
        let Some(s) = s else {
            return 0;
        };
        let Some(f) = s.current else {
            return 0;
        };
        let first = s.runs[f].is_empty();
        let run = sample(&s.workload, FORMS[f], first);
        if let Some(first) = s.runs[f].first() {
            measure::assert_counts(first, &run);
        }
        s.runs[f].push(run);
        s.runs[f].len() as u32
    })
}
#[no_mangle]
pub extern "C" fn bench_workload_finish() -> u32 {
    SESSION.with_borrow_mut(|s| {
        let Some(s) = s else {
            return 1;
        };
        let Some(f) = s.current else {
            return 1;
        };
        if s.runs[f].len().is_multiple_of(2) {
            return 1;
        }
        if s.reference.is_none() {
            let reference = FORMS.iter().find(|f| f.name() == REFERENCE).unwrap();
            s.reference =
                Some(measure::replay(&s.workload, *reference, true, &Monotonic::default()).written);
        }
        let differing = measure::differences(s.reference.as_ref().unwrap(), &s.runs[f][0].written);
        let runs = std::mem::take(&mut s.runs[f]);
        let summary = measure::summarize(&s.workload, &runs);
        let h = hash(&runs[0].written);
        s.row = vec![
            summary.mean,
            summary.p50.unwrap_or(f64::NAN),
            summary.p95.unwrap_or(f64::NAN),
            summary.p99.unwrap_or(f64::NAN),
            summary.max.unwrap_or(f64::NAN),
            summary.calls,
            summary.dispatches,
            differing as f64,
            (h >> 32) as f64,
            (h & 0xffff_ffff) as f64,
        ];
        s.row
            .extend(summary.phases.iter().map(|x| x.unwrap_or(f64::NAN)));
        s.series = summary
            .bins
            .iter()
            .enumerate()
            .flat_map(|(i, (us, calls))| {
                [
                    us.unwrap_or(f64::NAN),
                    *calls,
                    s.workload.phases[s.workload.warm_up + i * measure::BIN] as f64,
                ]
            })
            .collect();
        result(
            &s.workload,
            &[Measurement {
                form: FORMS[f].name().into(),
                summary,
                runs,
                differing,
            }],
        );
        0
    })
}
#[no_mangle]
pub extern "C" fn bench_workload_row(i: u32) -> f64 {
    SESSION.with_borrow(|s| {
        s.as_ref()
            .and_then(|s| s.row.get(i as usize).copied())
            .unwrap_or(f64::NAN)
    })
}
#[no_mangle]
pub extern "C" fn bench_workload_series() -> *const f64 {
    SESSION.with_borrow(|s| s.as_ref().map_or(std::ptr::null(), |s| s.series.as_ptr()))
}
#[no_mangle]
pub extern "C" fn bench_workload_series_len() -> u32 {
    SESSION.with_borrow(|s| s.as_ref().map_or(0, |s| s.series.len() as u32))
}
#[no_mangle]
pub extern "C" fn bench_measure(
    n: u32,
    scenario: u32,
    form: u32,
    warm_up: u32,
    ticks: u32,
    samples: u32,
) -> u32 {
    let (Some(scenario), Some(form)) = (
        Scenario::ALL.get(scenario as usize),
        FORMS.get(form as usize),
    ) else {
        return 1;
    };
    if n == 0 || ticks == 0 || samples.is_multiple_of(2) {
        return 1;
    }
    let workload = scenario.workload(n as usize, warm_up as usize, ticks as usize);
    let runs: Vec<_> = (0..samples)
        .map(|s| sample(&workload, *form, s == 0))
        .collect();
    let reference = FORMS.iter().find(|f| f.name() == REFERENCE).unwrap();
    let expected = measure::replay(&workload, *reference, true, &Monotonic::default());
    let differing = measure::differences(&expected.written, &runs[0].written);
    let h = hash(&runs[0].written);
    let summary = measure::summarize(&workload, &runs);
    let mut means: Vec<_> = runs.iter().map(|r| r.mean_us).collect();
    means.sort_by(f64::total_cmp);
    ROW.set([
        measure::quantile(&means, 0.5),
        measure::quantile(&means, 0.25),
        measure::quantile(&means, 0.75),
        summary.calls,
        summary.dispatches,
        (h >> 32) as f64,
        (h & 0xffff_ffff) as f64,
    ]);
    result(
        &workload,
        &[Measurement {
            form: form.name().into(),
            summary,
            runs,
            differing,
        }],
    );
    0
}
#[no_mangle]
pub extern "C" fn bench_row(i: u32) -> f64 {
    ROW.get().get(i as usize).copied().unwrap_or(f64::NAN)
}

#[no_mangle]
pub extern "C" fn bench_clock_step_us(us: f64) -> u32 {
    if !us.is_finite() || us < 0.0 {
        return 1;
    }
    STEP_US.set(us);
    0
}
fn defaults() -> &'static str {
    static DEFAULTS: OnceLock<String> = OnceLock::new();
    DEFAULTS.get_or_init(|| {
        serde_json::json!({"ticks": DEFAULT_TICKS, "warmUp": DEFAULT_WARM_UP,
        "sizes": SIZES, "reference": REFERENCE})
        .to_string()
    })
}
#[no_mangle]
pub extern "C" fn bench_defaults() -> *const u8 {
    defaults().as_ptr()
}
#[no_mangle]
pub extern "C" fn bench_defaults_len() -> u32 {
    defaults().len() as u32
}

#[cfg(feature = "bench-check")]
#[no_mangle]
pub extern "C" fn bench_check_virtual(step: f64, seed: u32, samples: u32) {
    RESULT.set(
        serde_json::to_vec(&crate::checks::virtual_run(step, seed, samples as usize)).unwrap(),
    );
}

#[no_mangle]
pub extern "C" fn bench_scale_name(n: u32, scenario: u32) -> u32 {
    let Some(scenario) = Scenario::ALL.get(scenario as usize) else {
        return 1;
    };
    RESULT.set(serde_json::to_vec(&crate::scale_name(scenario.name(), n as usize)).unwrap());
    0
}
