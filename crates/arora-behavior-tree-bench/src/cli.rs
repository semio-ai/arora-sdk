//! Thin native front ends over the same library used by WASI.
use crate::{
    builtins,
    clock::Monotonic,
    forms,
    measure::{self, Measurement},
    model::Workload,
    scale::{Scenario, SIZES},
    scale_name, DEFAULT_TICKS, DEFAULT_WARM_UP,
};
use std::{collections::BTreeMap, path::Path};

fn or_exit<T>(result: Result<T, String>) -> T {
    result.unwrap_or_else(|message| {
        eprintln!("{message}");
        std::process::exit(1);
    })
}

fn time(value: Option<f64>) -> String {
    value.map_or_else(|| "unresolved".into(), |x| format!("{x:.3}"))
}

pub fn workload_table(workload: &Workload, rows: &[Measurement]) -> String {
    let mut out = format!(
        "{} calls, {} ticks at {:.0} Hz, warm-up={}; µs per tick includes host writes\n",
        workload
            .nodes
            .0
            .iter()
            .filter(|n| !workload.attached.contains(&n.id))
            .count(),
        workload.events.len(),
        1.0 / workload.dt,
        workload.warm_up
    );
    out += "| form | mean us | p50 | p95 | p99 | max | calls | dispatches |";
    for phase in &workload.phase_names {
        out += &format!(" {phase} us |");
    }
    out += " ticks differing |\n|---|";
    out += &"---:|".repeat(8 + workload.phase_names.len());
    out += "\n";
    for m in rows {
        let s = &m.summary;
        out += &format!(
            "| {} | {:.3} | {} | {} | {} | {} | {:.3} | {:.3} |",
            m.form,
            s.mean,
            time(s.p50),
            time(s.p95),
            time(s.p99),
            time(s.max),
            s.calls,
            s.dispatches
        );
        for phase in &s.phases {
            out += &format!(" {} |", time(*phase));
        }
        out += &format!(" {} |\n", m.differing);
    }
    out
}

pub fn scale_header() -> &'static str {
    "| N | scenario | form | mean us | p50 | p95 | p99 | max | calls | dispatches | ticks differing |\n|---:|---|---|---:|---:|---:|---:|---:|---:|---:|---:|\n"
}
pub fn scale_rows(n: usize, scenario: Scenario, rows: &[Measurement]) -> String {
    rows.iter()
        .map(|m| {
            let s = &m.summary;
            format!(
                "| {n} | {} | {} | {:.3} | {} | {} | {} | {} | {:.3} | {:.3} | {} |\n",
                scenario.name(),
                m.form,
                s.mean,
                time(s.p50),
                time(s.p95),
                time(s.p99),
                time(s.max),
                s.calls,
                s.dispatches,
                m.differing
            )
        })
        .collect()
}

fn number(args: &[String], i: usize, default: usize) -> usize {
    args.get(i)
        .map_or(default, |a| a.parse().expect("integer argument"))
}
fn chosen(variable: &str) -> Vec<&'static dyn forms::Form> {
    forms::selected(std::env::var(variable).ok().as_deref()).unwrap()
}
fn agreement(rows: &[Measurement]) {
    assert!(
        rows.iter().all(|m| m.differing == 0),
        "written values differ from detached"
    );
}
fn write(path: impl AsRef<Path>, json: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(json).unwrap()).unwrap();
}
fn args() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| a != "--bench")
        .collect()
}

pub fn workload_main() {
    let args = args();
    let name = args.first().map_or("synthetic", String::as_str);
    let workload = builtins::workload(name).unwrap_or_else(|| or_exit(Workload::load(name)));
    let samples = number(&args, 1, 7);
    let rows = or_exit(measure::measure(
        &workload,
        &chosen("WORKLOAD_ONLY"),
        samples,
        &Monotonic::default(),
    ));
    print!("{}", workload_table(&workload, &rows));
    if let Some(path) = args
        .get(2)
        .cloned()
        .or_else(|| std::env::var("WORKLOAD_TIMES").ok())
    {
        write(path, &measure::record(&workload, &rows));
    }
    agreement(&rows);
}

pub fn scale_main() {
    let args = args();
    let ticks = number(&args, 0, DEFAULT_TICKS);
    let samples = number(&args, 1, 7);
    let warm_up = std::env::var("SCALE_WARM_UP").map_or(DEFAULT_WARM_UP, |x| x.parse().unwrap());
    let sizes = std::env::var("SCALE_SIZES")
        .ok()
        .map(|s| s.split(',').map(|n| n.parse().unwrap()).collect::<Vec<_>>())
        .unwrap_or_else(|| SIZES.to_vec());
    let forms = chosen("SCALE_FORMS");
    if let Some(only) = args.get(2) {
        assert!(
            valid_cell(only, &sizes, &forms),
            "unknown or unselected exact scale cell: {only}"
        );
    }
    let mut records = Vec::new();
    print!("{}", scale_header());
    for n in sizes {
        for scenario in Scenario::ALL {
            let selected: Vec<_> = forms
                .iter()
                .copied()
                .filter(|form| {
                    args.get(2).is_none_or(|only| {
                        only == &format!("{n}/{}/{}", scenario.name(), form.name())
                    })
                })
                .collect();
            if selected.is_empty() {
                continue;
            }
            let workload = scenario.workload(n, warm_up, ticks);
            let rows = or_exit(measure::measure(
                &workload,
                &selected,
                samples,
                &Monotonic::default(),
            ));
            print!("{}", scale_rows(n, scenario, &rows));
            records.push(serde_json::json!({"n": n, "scenario": scenario.name(), "record": measure::record(&workload, &rows)}));
            agreement(&rows);
        }
    }
    assert!(
        !records.is_empty(),
        "unknown or unselected exact scale cell"
    );
    if let Ok(path) = std::env::var("SCALE_TIMES") {
        write(path, &serde_json::json!(records));
    }
}

/// One process measures every requested workload and scale cell in a round.
/// Usage: bench-kit <out.json> [workload.json ...]; configuration is by env.
pub fn round_main() {
    let args = args();
    let out = args.first().expect("output JSON path");
    let samples = std::env::var("BENCH_SAMPLES").map_or(1, |s| s.parse().unwrap());
    let ticks = std::env::var("BENCH_TICKS").map_or(DEFAULT_TICKS, |s| s.parse().unwrap());
    let warm_up = std::env::var("BENCH_WARM_UP").map_or(DEFAULT_WARM_UP, |s| s.parse().unwrap());
    let sizes = std::env::var("BENCH_SIZES")
        .ok()
        .map(|s| s.split(',').map(|n| n.parse().unwrap()).collect::<Vec<_>>())
        .unwrap_or_else(|| SIZES.to_vec());
    let forms = chosen("BENCH_FORMS");
    let clock = Monotonic::default();
    let mut tests = BTreeMap::new();
    let mut run = |name: String, kind, n, workload: Workload| {
        assert!(!tests.contains_key(&name), "duplicate test name: {name}");
        let rows = or_exit(measure::measure(&workload, &forms, samples, &clock));
        agreement(&rows);
        tests.insert(name, serde_json::json!({"kind": kind, "size": n, "record": measure::record(&workload, &rows)}));
    };
    for name in builtins::NAMES {
        run(
            (*name).into(),
            "workload",
            0,
            builtins::workload(name).unwrap(),
        );
    }
    for path in args.iter().skip(1) {
        let name = Path::new(path)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        run(name, "workload", 0, or_exit(Workload::load(path)));
    }
    for n in sizes {
        for s in Scenario::ALL {
            run(
                scale_name(s.name(), n),
                "scale",
                n,
                s.workload(n, warm_up, ticks),
            );
        }
    }
    write(out, &serde_json::json!({"schema": 1, "tests": tests}));
}

fn valid_cell(only: &str, sizes: &[usize], forms: &[&dyn forms::Form]) -> bool {
    sizes.iter().any(|n| {
        Scenario::ALL.iter().any(|s| {
            forms
                .iter()
                .any(|f| only == format!("{n}/{}/{}", s.name(), f.name()))
        })
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn exact_scale_selection_rejects_unknown_cells() {
        assert!(super::valid_cell(
            "10/clock/detached",
            &[10],
            crate::forms::FORMS
        ));
        assert!(!super::valid_cell(
            "10/clok/detached",
            &[10],
            crate::forms::FORMS
        ));
        assert!(!super::valid_cell(
            "30/clock/detached",
            &[10],
            crate::forms::FORMS
        ));
    }
}
