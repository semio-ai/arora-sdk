//! Playback against the curve: every fixture-like and generated clip, played
//! by a player stepped at 100 Hz and baked at 100 Hz, lands on the track's
//! exact curve at every frame.
//!
//! The curve is computed here independently of the crate's evaluator, from
//! the same resolved control points: the keypoints in time order, segments of
//! no duration skipped, each number segment's time solved by 64 bisections
//! in milliseconds, a boolean or string holding its segment's start value,
//! the last keypoint playing at its instant. What the engine adds on the way
//! — the player's clock, the instance's timing, the output table, the bake's
//! frame times — is what this holds to it.
//!
//! The generator covers what a clip can hold: number, string and boolean
//! tracks; every easing, both inferred tangents and explicit handles,
//! including ones the clamps must correct; keypoints out of order, sharing a
//! time, or repeating a value.

use arora_animation::clip::{Clip, Easing, Keypoint, Track, Transition, Transitions};
use arora_animation::resolve::{resolve_segment, Point, SegmentInput};
use arora_animation::{Engine, LoopMode, Nanos, Timing, Value};

const FRAME_NS: Nanos = 10_000_000;
const VALUE_TOLERANCE: f64 = 1e-9;

#[test]
fn every_generated_clip_plays_on_its_curve() {
    for seed in 1..=120 {
        let clip = random_clip(seed);
        check(&format!("seed {seed}"), &clip);
    }
}

#[test]
fn a_clip_of_long_and_steep_segments_plays_on_its_curve() {
    // A 20 ms ramp at the end of a 600 s clip, 2 ms segments, and values
    // far from 0: where single-precision time and values lose the curve.
    let mut keypoints = vec![
        keypoint(0, Value::Number(100_000.0), None, None),
        keypoint(599_980, Value::Number(100_000.0), None, None),
        keypoint(600_000, Value::Number(100_001.0), None, None),
    ];
    for i in 0..30 {
        keypoints.push(keypoint(
            1_000 + 2 * i,
            Value::Number(if i % 2 == 0 { -5.0 } else { 5.0 }),
            Some(Transition::Easing(Easing::Linear)),
            Some(Transition::Easing(Easing::Linear)),
        ));
    }
    let clip = Clip {
        name: "long".into(),
        length: Some(600_000 * 1_000_000),
        tracks: vec![Track {
            id: "t".into(),
            name: "t".into(),
            target: "x".into(),
            keypoints,
        }],
    };
    check("long", &clip);
}

fn keypoint(
    ms: i64,
    value: Value,
    arrive: Option<Transition>,
    leave: Option<Transition>,
) -> Keypoint {
    Keypoint {
        id: format!("k{ms}"),
        time: ms * 1_000_000,
        value,
        transitions: Transitions {
            arrive,
            leave,
            pairing: None,
        },
    }
}

/// Plays `clip` from 0 past its end, stepped at 100 Hz, and bakes it at
/// 100 Hz; every frame of both is held to the oracle.
fn check(name: &str, clip: &Clip) {
    let mut engine = Engine::new();
    let anim = engine.load(clip.clone(), vec![]);
    let player = engine.create_player("p");
    engine.set_mode(player, LoopMode::Once);
    engine
        .add_instance(player, anim, 1.0, Timing::default())
        .unwrap();
    engine.play(player);
    let keys = engine.output_keys().1.to_vec();
    let end = clip.end();
    let frames = end / FRAME_NS + 3;
    let oracles: Vec<(&Track, Oracle)> = played_tracks(clip)
        .into_iter()
        .map(|t| (t, Oracle::new(t)))
        .collect();
    for n in 0..frames {
        let t = n * FRAME_NS;
        let outputs = engine.step(t, if n == 0 { 0 } else { FRAME_NS });
        let position = engine.players()[0].position;
        assert_eq!(position, t.min(end), "{name}: the playhead at {t} ns");
        for (track, oracle) in &oracles {
            let at = keys
                .iter()
                .position(|k| *k == track.target)
                .expect("an output");
            let played = outputs[at].sample.as_ref().expect("a value").value.clone();
            expect_on_curve(
                &format!("{name}: {} stepped to {position} ns", track.id),
                &played,
                oracle,
                position as f64,
            );
        }
    }
    let bake = engine.bake(anim, 100.0, None, None).expect("a bake");
    for (track, oracle) in &oracles {
        let (_, samples) = bake
            .tracks
            .iter()
            .find(|(target, _)| *target == track.target)
            .expect("baked");
        for (i, sample) in samples.iter().enumerate() {
            let t = i as Nanos * FRAME_NS;
            expect_on_curve(
                &format!("{name}: {} baked at {t} ns", track.id),
                &sample.value,
                oracle,
                t as f64,
            );
        }
    }
}

/// The tracks that play: the first on each target, with keypoints.
fn played_tracks(clip: &Clip) -> Vec<&Track> {
    let mut seen = std::collections::HashSet::new();
    clip.tracks
        .iter()
        .filter(|t| seen.insert(t.target.clone()))
        .filter(|t| !t.keypoints.is_empty())
        .collect()
}

fn expect_on_curve(at: &str, got: &Value, oracle: &Oracle, t: f64) {
    let expected = oracle.at(t);
    match (got, &expected) {
        (Value::Number(v), Value::Number(e)) => {
            let scale = oracle.magnitude;
            if (v - e).abs() <= VALUE_TOLERANCE * scale {
                return;
            }
            // Where a tangent stands vertical, the value turns within a
            // rounding of the time: hold it to the curve within 1 µs.
            let around = [t - 1e3, t + 1e3].map(|t| oracle.at(t).as_number().unwrap());
            let (low, high) = (
                around[0].min(around[1]).min(*e),
                around[0].max(around[1]).max(*e),
            );
            assert!(
                *v >= low - VALUE_TOLERANCE * scale && *v <= high + VALUE_TOLERANCE * scale,
                "{at}: {v}, expected {e}"
            );
        }
        (got, expected) => assert_eq!(got, expected, "{at}"),
    }
}

/// The exact curve of one track, computed independently of the crate's
/// evaluator.
struct Oracle {
    points: Vec<(f64, Value)>,
    segments: Vec<OracleSegment>,
    magnitude: f64,
}

struct OracleSegment {
    start: f64,
    end: f64,
    from: usize,
    to: usize,
    /// Milliseconds and values.
    bezier: Option<([f64; 4], [f64; 4])>,
}

impl Oracle {
    fn new(track: &Track) -> Oracle {
        let mut points: Vec<&Keypoint> = track.keypoints.iter().collect();
        points.sort_by_key(|k| k.time);
        let ms = |k: &Keypoint| k.time as f64 / 1e6;
        let number = |i: usize| {
            points
                .get(i)
                .map(|k| Point::new(ms(k), k.value.as_number().unwrap_or(0.0)))
        };
        let mut segments = Vec::new();
        for i in 0..points.len().saturating_sub(1) {
            let (a, b) = (points[i], points[i + 1]);
            if a.time == b.time {
                continue;
            }
            let bezier = a.value.as_number().map(|_| {
                let resolved = resolve_segment(&SegmentInput {
                    start: number(i).unwrap(),
                    end: number(i + 1).unwrap(),
                    leave: a.transitions.leave,
                    arrive: b.transitions.arrive,
                    before: i.checked_sub(1).and_then(number),
                    after: number(i + 2),
                });
                let [cp1, cp2] = resolved.effective;
                (
                    [ms(a), cp1.t, cp2.t, ms(b)],
                    [
                        a.value.as_number().unwrap(),
                        cp1.v,
                        cp2.v,
                        b.value.as_number().unwrap(),
                    ],
                )
            });
            segments.push(OracleSegment {
                start: ms(a),
                end: ms(b),
                from: i,
                to: i + 1,
                bezier,
            });
        }
        let magnitude = points
            .iter()
            .filter_map(|k| k.value.as_number())
            .fold(1.0_f64, |m, v| m.max(v.abs()));
        Oracle {
            points: points.iter().map(|k| (ms(k), k.value.clone())).collect(),
            segments,
            magnitude,
        }
    }

    /// The track at `t` nanoseconds, by the reference results' rules: what
    /// the first segment starts from before it, what the last ends at after
    /// it, and with no segment the first value before the keypoints' time and
    /// the last from it on.
    fn at(&self, t: f64) -> Value {
        let t = t / 1e6;
        let Some(first) = self.segments.first() else {
            let index = if t < self.points[0].0 {
                0
            } else {
                self.points.len() - 1
            };
            return self.points[index].1.clone();
        };
        if t <= first.start {
            return self.points[first.from].1.clone();
        }
        let last = self.segments.last().unwrap();
        if t >= last.end {
            return self.points[last.to].1.clone();
        }
        let segment = self.segments.iter().find(|s| t <= s.end).unwrap();
        let Some((xs, ys)) = segment.bezier else {
            return self.points[segment.from].1.clone();
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..64 {
            let mid = 0.5 * (lo + hi);
            if cubic(&xs, mid) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Value::Number(cubic(&ys, 0.5 * (lo + hi)))
    }
}

fn cubic(p: &[f64; 4], s: f64) -> f64 {
    let m = 1.0 - s;
    p[0] * m * m * m + 3.0 * p[1] * m * m * s + 3.0 * p[2] * m * s * s + p[3] * s * s * s
}

// --- the generator --------------------------------------------------------

/// A seeded clip. Every keypoint is at an odd multiple of 5 ms, so a 100 Hz
/// frame never lands on one.
fn random_clip(seed: u32) -> Clip {
    let random = &mut mulberry32(seed);
    let mut tracks = Vec::new();
    let track_count = 1 + int(random, 4);
    for t in 0..track_count {
        let kind = ["number", "number", "number", "string", "boolean"][int(random, 5)];
        let mut keypoints = Vec::new();
        let point_count = 1 + int(random, 7);
        let mut previous: Option<Value> = None;
        for p in 0..point_count {
            let stamp = 5 + 10 * int(random, 300) as i64;
            let value = match kind {
                "string" => Value::Text(["idle", "smile", "blink", ""][int(random, 4)].to_string()),
                "boolean" => Value::Bool(random() < 0.5),
                _ => match previous.as_ref().and_then(Value::as_number) {
                    Some(previous) if random() < 0.25 => Value::Number(if random() < 0.5 {
                        previous
                    } else {
                        previous + previous * 1e-9
                    }),
                    _ => Value::Number((random() - 0.5) * 100.0),
                },
            };
            previous = Some(value.clone());
            let transitions = if random() < 0.15 {
                Transitions::default()
            } else {
                // Studio's interpolator cannot offset a string by a handle.
                let explicit = kind != "string";
                let arrive = random_transition(random, -1.0, explicit);
                let leave = random_transition(random, 1.0, explicit);
                Transitions {
                    arrive,
                    leave,
                    pairing: None,
                }
            };
            keypoints.push(Keypoint {
                id: format!("k{p}"),
                time: stamp * 1_000_000,
                value,
                transitions,
            });
        }
        if random() < 0.7 {
            keypoints.sort_by_key(|k| k.time);
        }
        tracks.push(Track {
            id: format!("track-{seed}-{t}"),
            name: String::new(),
            target: format!("animatable-{t}"),
            keypoints,
        });
    }
    let viewport = if random() < 0.2 {
        Some(500 + int(random, 4000) as i64)
    } else {
        None
    };
    let content = tracks
        .iter()
        .flat_map(|t| t.keypoints.iter().map(|k| k.time / 1_000_000))
        .max()
        .unwrap_or(0);
    // Studio's length: the content, the viewport, and at least a second.
    let length_ms = content.max(viewport.unwrap_or(0)).max(1_000);
    Clip {
        name: format!("animation {seed}"),
        length: Some(length_ms * 1_000_000),
        tracks,
    }
}

const EASINGS: [Easing; 9] = [
    Easing::Sine,
    Easing::Cubic,
    Easing::Quint,
    Easing::Circ,
    Easing::Quad,
    Easing::Quart,
    Easing::Expo,
    Easing::Back,
    Easing::Linear,
];

/// A side's transition: none, an easing, an inferred tangent, or an
/// explicit handle pointing the wrong way one time in ten.
fn random_transition(
    random: &mut impl FnMut() -> f64,
    direction: f64,
    explicit: bool,
) -> Option<Transition> {
    let roll = random();
    if roll < 0.15 {
        return None;
    }
    if roll < 0.45 || !explicit {
        return Some(Transition::Easing(EASINGS[int(random, EASINGS.len())]));
    }
    if roll < 0.6 {
        return Some([Transition::Auto, Transition::AutoClamped][int(random, 2)]);
    }
    let sign = if random() < 0.1 {
        -direction
    } else {
        direction
    };
    let dt = sign * random() * 400.0 * 1e6;
    let dv = (random() - 0.5) * 60.0;
    Some(Transition::Handle { dt, dv })
}

/// `Math.floor(random() * n)`.
fn int(random: &mut impl FnMut() -> f64, n: usize) -> usize {
    (random() * n as f64).floor() as usize
}

/// mulberry32, as Studio's generator seeds its animations.
fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b_79f5);
        let mut t = a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}
