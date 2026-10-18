//! The reference results: every case in `fixtures/conformance/` evaluated
//! through the crate, its numbers within the stated tolerance and its
//! booleans and strings exact.
//!
//! Each case is a stored Studio animation, an instance of it (an offset, a
//! timescale, whether it plays) and the value and world-time velocity of
//! every track at a list of world times. The fixtures' README states the
//! rules they were computed by.

mod support;

use std::path::Path;

use arora_animation::{Curve, Timing, Value};
use serde_json::Value as Json;

/// A number is within this much of the reference, relative to the larger of
/// its track's value range and 1.
const VALUE_TOLERANCE: f64 = 1e-9;
/// A velocity is within this much of the reference, relative to the larger
/// of its magnitude and the track's range per second over its shortest
/// segment.
const VELOCITY_TOLERANCE: f64 = 1e-9;

#[derive(Default, Debug)]
struct Worst {
    value: f64,
    velocity: f64,
    samples: usize,
}

#[test]
fn every_reference_case_plays_as_recorded() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/conformance");
    let mut cases: Vec<_> = std::fs::read_dir(&dir)
        .expect("the fixtures")
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    cases.sort();
    assert!(cases.len() >= 68, "{} cases", cases.len());
    let mut worst = Worst::default();
    for path in &cases {
        let case: Json = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        check_case(
            path.file_name().unwrap().to_str().unwrap(),
            &case,
            &mut worst,
        );
    }
    eprintln!(
        "{} samples; worst relative error: value {:e}, velocity {:e}",
        worst.samples, worst.value, worst.velocity
    );
}

fn check_case(name: &str, case: &Json, worst: &mut Worst) {
    let (clip, muted) = support::clip_of_studio_v2(&case["animation"]);
    let instance = &case["instance"];
    let active = instance["active"] != Json::Bool(false);
    let timing = Timing {
        offset: arora_animation::time::nanos_from_millis(instance["offset"].as_f64().unwrap())
            .unwrap(),
        rate: instance["timescale"].as_f64().unwrap(),
    };
    let curves: Vec<(&str, Curve, f64, f64)> = clip
        .tracks
        .iter()
        .map(|track| {
            let numbers: Vec<f64> = track
                .keypoints
                .iter()
                .filter_map(|k| k.value.as_number())
                .collect();
            let range = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - numbers.iter().copied().fold(f64::INFINITY, f64::min);
            let mut times: Vec<i64> = track.keypoints.iter().map(|k| k.time).collect();
            times.sort();
            let shortest = times
                .windows(2)
                .map(|w| w[1] - w[0])
                .filter(|d| *d > 0)
                .min()
                .unwrap_or(1);
            let range = if range.is_finite() {
                range.max(1.0)
            } else {
                1.0
            };
            let slope = range / (shortest as f64 / 1e9) * timing.rate;
            (track.id.as_str(), Curve::new(track), range, slope)
        })
        .collect();
    for sample in case["samples"].as_array().unwrap() {
        let world_ms = sample["worldTimeMs"].as_f64().unwrap();
        let local = timing.local(world_ms * 1e6);
        for (id, curve, range, slope) in &curves {
            let expected = &sample["tracks"][*id];
            let at = format!("{name}: track {id} at {world_ms} ms");
            let played = active && !muted.iter().any(|m| m == id);
            let got = if played { curve.sample(local) } else { None };
            let Some(got) = got else {
                assert!(
                    expected["value"].is_null(),
                    "{at}: plays nothing, expected {expected}"
                );
                assert!(expected["velocity"].is_null(), "{at}");
                continue;
            };
            worst.samples += 1;
            match (&got.value, &expected["value"]) {
                (Value::Number(v), Json::Number(e)) => {
                    let e = e.as_f64().unwrap();
                    let error = (v - e).abs() / range;
                    worst.value = worst.value.max(error);
                    assert!(
                        error <= VALUE_TOLERANCE,
                        "{at}: {v}, expected {e} ({error:e} of its range)"
                    );
                    let velocity = got.velocity.unwrap() * timing.rate;
                    let expected = expected["velocity"].as_f64().unwrap();
                    let scale = expected.abs().max(*slope);
                    let error = (velocity - expected).abs() / scale;
                    worst.velocity = worst.velocity.max(error);
                    assert!(
                        error <= VELOCITY_TOLERANCE,
                        "{at}: velocity {velocity}, expected {expected} ({error:e})"
                    );
                }
                (Value::Bool(v), Json::Bool(e)) => {
                    assert_eq!(v, e, "{at}");
                    assert!(expected["velocity"].is_null(), "{at}");
                }
                (Value::Text(v), Json::String(e)) => {
                    assert_eq!(v, e, "{at}");
                    assert!(expected["velocity"].is_null(), "{at}");
                }
                (got, expected) => panic!("{at}: {got:?}, expected {expected}"),
            }
        }
    }
}
