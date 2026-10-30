//! Fixtures compiled only for tests and the explicit WASI check build.
use crate::{
    clock::Clock,
    forms::{self, Fixture, Form, Nodes},
    measure,
    model::Workload,
    scale::Scenario,
};
use std::cell::Cell;

pub struct Wrong;
impl Form for Wrong {
    fn name(&self) -> &'static str {
        "test-wrong"
    }
    fn build(&self, workload: &Workload) -> Nodes {
        let mut nodes = workload.nodes.clone();
        nodes.0[0].function = arora_behavior_tree::nodes::RUN_FUNCTION_ID;
        nodes.0[0].children = None;
        nodes
    }
    fn fixture_counts(&self, _fixture: Fixture) -> (u64, u64) {
        (0, 0)
    }
}

struct VirtualClock {
    time: Cell<f64>,
    seed: Cell<u32>,
    step: f64,
}
impl Clock for VirtualClock {
    fn now(&self) -> f64 {
        if self.step == 0.0 {
            self.time.get()
        } else {
            (self.time.get() / self.step).floor() * self.step
        }
    }
    fn step_us(&self) -> f64 {
        self.step
    }
    fn tick_finished(&self) {
        let seed = self
            .seed
            .get()
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
        self.seed.set(seed);
        // Real work advances time, independently of how many clock reads occur.
        // U(0,2) has a known expected duration of exactly 1 microsecond per tick.
        self.time
            .set(self.time.get() + f64::from(seed) / 4294967296.0 * 2.0);
    }
}
pub fn virtual_run(step: f64, seed: u32, samples: usize) -> serde_json::Value {
    let clock = VirtualClock {
        time: Cell::new(f64::from(seed % 100)),
        seed: Cell::new(seed),
        step,
    };
    let workload = Scenario::Settled.workload(1, 7, 53);
    let runs = (0..samples)
        .map(|s| {
            measure::replay_adaptive(&workload, forms::FORMS[0], s == 0, &clock, step * 50.0)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let m = measure::Measurement {
        form: forms::FORMS[0].name().into(),
        summary: measure::summarize(&workload, &runs),
        runs,
        differing: 0,
    };
    measure::record(&workload, &[m])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantized_clock_resolves_replay_mean_without_stopping_bias() {
        for (step, samples) in [(0.0, 3), (100.0, 1), (100.0, 3)] {
            let means = (1..=128)
                .map(|seed| {
                    virtual_run(step, seed * 7919, samples)["forms"]["detached"]["summary"]["mean"]
                        .as_f64()
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let mean = measure::mean(means.into_iter());
            println!("step={step} us, samples={samples}: mean={mean:.6} us, truth=1.000000 us");
            assert!((mean - 1.0).abs() < 0.005, "coarse-clock mean {mean}");
        }
        let record = virtual_run(100.0, 12345, 3);
        let row = &record["forms"]["detached"];
        assert_eq!(row["timingMode"][0], "replay");
        assert!(row["summary"]["p50"].is_null());
        assert!(row["summary"]["max"].is_null());
        assert!(row["summary"]["phases"][0].is_null());
        assert!(row["summary"]["bins"][0][0].is_null());
    }

    #[test]
    fn coarse_replays_take_two_reads_and_fix_their_count_from_a_pilot() {
        struct ClockWithReads {
            time: Cell<f64>,
            reads: Cell<usize>,
        }
        impl Clock for ClockWithReads {
            fn now(&self) -> f64 {
                self.reads.set(self.reads.get() + 1);
                (self.time.get() / 100.0).floor() * 100.0
            }
            fn step_us(&self) -> f64 {
                100.0
            }
            fn tick_finished(&self) {
                self.time.set(self.time.get() + 1.0);
            }
        }
        let clock = ClockWithReads {
            time: Cell::new(13.0),
            reads: Cell::new(0),
        };
        let workload = Scenario::Settled.workload(1, 0, 53);
        let run =
            measure::replay_adaptive(&workload, forms::FORMS[0], true, &clock, 5000.0).unwrap();
        assert_eq!(run.repetitions, 50); // unresolved pilot uses its 100 us step
        assert_eq!(clock.reads.get(), 2 + 2 * run.repetitions);
        assert_eq!(run.replay_us.len(), run.repetitions);
        assert_eq!(run.written.len(), 53);
    }

    /// A replay whose length is a whole number of clock steps starts every
    /// interval at the same phase, so its quantized readings never average out.
    #[test]
    fn a_replay_locked_to_the_clock_step_leaves_the_mean_unresolved() {
        struct Locked(Cell<f64>);
        impl Clock for Locked {
            fn now(&self) -> f64 {
                (self.0.get() / 100.0).floor() * 100.0
            }
            fn step_us(&self) -> f64 {
                100.0
            }
            fn tick_finished(&self) {
                self.0.set(self.0.get() + 1.0);
            }
        }
        let workload = Scenario::Settled.workload(1, 99, 100);
        let clock = Locked(Cell::new(0.0));
        let run =
            measure::replay_adaptive(&workload, forms::FORMS[0], false, &clock, 5000.0).unwrap();
        assert!(
            run.replay_us.iter().all(|&x| x == 100.0),
            "{:?}",
            run.replay_us
        );
        assert!(run.mean_us.is_nan(), "{}", run.mean_us);
        // The same workload on a clock whose phase moves resolves its mean.
        let clock = Locked(Cell::new(13.0));
        let shifted = Scenario::Settled.workload(1, 99, 113);
        let run =
            measure::replay_adaptive(&shifted, forms::FORMS[0], false, &clock, 5000.0).unwrap();
        assert!(run.mean_us.is_finite());
    }

    #[test]
    fn wrong_form_is_rejected_alone_and_with_reference() {
        let mut scatter = crate::synthetic::json();
        scatter["text"] = "scatter(keys: str[\"dynamic\"], values: f32[1])".into();
        for workload in [
            crate::synthetic::workload(),
            Workload::parse(&scatter.to_string()).unwrap(),
        ] {
            for selected in [vec![&Wrong as &dyn Form], vec![forms::FORMS[0], &Wrong]] {
                let rows =
                    measure::measure(&workload, &selected, 1, &crate::clock::Monotonic::default())
                        .unwrap();
                assert!(rows.last().unwrap().differing > 0);
            }
        }
    }
}
