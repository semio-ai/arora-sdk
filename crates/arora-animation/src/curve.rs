//! A track compiled for evaluation: its keypoints in time order and the
//! segments between them, each number segment a cubic Bézier in (time,
//! value) whose control points [`crate::resolve`] placed.
//!
//! **The step rule.** A boolean or string holds the value of the keypoint
//! that starts its segment. At an interior keypoint's own instant the
//! segment that ends there still holds; just after it, that keypoint's value
//! plays. The last keypoint plays at its instant, and so does every value
//! held after it.
//!
//! **Evaluation is exact**: a number segment's time is solved for the curve's
//! parameter to double precision, and its value and velocity come from the
//! Bézier itself, not from a sampled polyline.
//!
//! Before the first keypoint the track holds its first value, after the last
//! its last, with no velocity. A segment of no duration (keypoints sharing a
//! time) plays nothing of its own: the later keypoint takes over from it. At
//! the ends the segments decide, as in the reference results: before the
//! first segment the value it starts from holds, after the last the value it
//! ends at; with every keypoint at one time, the first value before it and
//! the last from it on.

use crate::clip::{Keypoint, Track};
use crate::resolve::{self, Point, SegmentInput};
use crate::time::{Nanos, NANOS_PER_MILLI, NANOS_PER_SECOND};
use crate::value::{Kind, Sample, Value};

/// A track, ready to evaluate at any clip-local time.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    kind: Option<Kind>,
    /// The keypoints' values, in time order.
    values: Vec<Value>,
    /// The earliest keypoint's time: with no segment, before it the first
    /// value holds and from it on the last.
    start: Nanos,
    segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq)]
struct Segment {
    start: Nanos,
    end: Nanos,
    /// Indices into the curve's values.
    from: usize,
    to: usize,
    /// For a number segment, the curve: times in nanoseconds from `start`.
    bezier: Option<Bezier>,
}

/// A cubic Bézier: `x` in nanoseconds from its segment's start (`x[0]` is 0,
/// `x[3]` the segment's duration), `y` in value units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bezier {
    pub x: [f64; 4],
    pub y: [f64; 4],
}

/// One segment of a number track as it plays: where it starts and ends, and
/// its curve in absolute clip-local nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayedSegment {
    pub start: Nanos,
    pub end: Nanos,
    /// The control points in clip-local nanoseconds and value units.
    pub control_points: [(f64, f64); 4],
}

impl Curve {
    /// Compiles `track`. Its keypoints are taken in time order, and
    /// keypoints sharing a time in the order the track gives them. The track
    /// is assumed valid ([`Track::validate`]): a number segment reads its
    /// keypoints' numbers.
    pub fn new(track: &Track) -> Curve {
        let mut order: Vec<&Keypoint> = track.keypoints.iter().collect();
        order.sort_by_key(|k| k.time);
        let kind = track.kind();
        let number = |i: usize| {
            order.get(i).map(|k| {
                Point::new(
                    k.time as f64 / NANOS_PER_MILLI as f64,
                    k.value.as_number().unwrap_or(0.0),
                )
            })
        };
        let mut segments = Vec::new();
        for i in 0..order.len().saturating_sub(1) {
            let (start, end) = (order[i], order[i + 1]);
            if start.time == end.time {
                continue;
            }
            let bezier = (kind == Some(Kind::Number)).then(|| {
                let resolved = resolve::resolve_segment(&SegmentInput {
                    start: number(i).expect("a segment's start"),
                    end: number(i + 1).expect("a segment's end"),
                    leave: start.transitions.leave,
                    arrive: end.transitions.arrive,
                    before: i.checked_sub(1).and_then(number),
                    after: number(i + 2),
                });
                bezier_of(start, end, resolved.effective)
            });
            segments.push(Segment {
                start: start.time,
                end: end.time,
                from: i,
                to: i + 1,
                bezier,
            });
        }
        Curve {
            kind,
            values: order.iter().map(|k| k.value.clone()).collect(),
            start: order.first().map_or(0, |k| k.time),
            segments,
        }
    }

    /// The kind of the track's values; `None` for a track with no keypoint,
    /// which plays nothing.
    pub fn kind(&self) -> Option<Kind> {
        self.kind
    }

    /// The track's segments as they play, in time order: each number
    /// segment's curve in absolute time. A boolean or string track has none.
    pub fn played_segments(&self) -> Vec<PlayedSegment> {
        self.segments
            .iter()
            .filter_map(|s| {
                let b = s.bezier?;
                let start = s.start as f64;
                Some(PlayedSegment {
                    start: s.start,
                    end: s.end,
                    control_points: std::array::from_fn(|i| (start + b.x[i], b.y[i])),
                })
            })
            .collect()
    }

    /// The track at clip-local time `t`, in nanoseconds; `None` for a track
    /// with no keypoint. A velocity is in value units per second of
    /// clip-local time.
    pub fn sample(&self, t: f64) -> Option<Sample> {
        let mut cursor = Cursor::default();
        self.sample_with(t, &mut cursor)
    }

    /// The track at each of `times`, in clip-local nanoseconds, appended to
    /// `out`: one evaluation of the segments for the whole batch. Fastest for
    /// times in increasing order, but any order is correct. Appends nothing
    /// for a track with no keypoint.
    pub fn sample_many(&self, times: &[f64], out: &mut Vec<Sample>) {
        if self.values.is_empty() {
            return;
        }
        out.reserve(times.len());
        let mut cursor = Cursor::default();
        for &t in times {
            out.extend(self.sample_with(t, &mut cursor));
        }
    }

    /// A number track at each of `times`: values and velocities written
    /// into `values` and `velocities` (each as long as `times`). Returns
    /// `false`, writing nothing, for a track that is not a number track.
    pub fn sample_numbers(
        &self,
        times: &[f64],
        values: &mut [f64],
        velocities: &mut [f64],
    ) -> bool {
        if self.kind != Some(Kind::Number) {
            return false;
        }
        assert!(values.len() >= times.len() && velocities.len() >= times.len());
        let mut cursor = Cursor::default();
        for (i, &t) in times.iter().enumerate() {
            let (value, velocity) = self.number_at(t, &mut cursor);
            values[i] = value;
            velocities[i] = velocity;
        }
        true
    }

    /// A number track drawn from `start` to `end` (clip-local nanoseconds)
    /// as `count` points `(time, value)` on the curve, appended to `out`, in
    /// increasing time: the first at `start`, the last at `end`. Where the
    /// range lies within one segment the points are evenly spaced in its
    /// Bézier's parameter, as a cubic is drawn, which solves for time only at
    /// the two ends; elsewhere they are evenly spaced in time. Each value is
    /// the one [`Curve::sample`] gives at that point's time, to rounding.
    /// Returns `false`, appending nothing, for a track that is not a number
    /// track.
    ///
    /// A range counts as within a segment when its ends are within half a
    /// nanosecond of the segment's: an editor holds a keypoint's time in
    /// fractional milliseconds, which the clip rounds to whole nanoseconds.
    pub fn polyline(&self, start: f64, end: f64, count: usize, out: &mut Vec<(f64, f64)>) -> bool {
        if self.kind != Some(Kind::Number) {
            return false;
        }
        out.reserve(count);
        let last = count.saturating_sub(1).max(1) as f64;
        let index = self.segments.partition_point(|s| s.end as f64 + 0.5 < end);
        let within = self
            .segments
            .get(index)
            .filter(|s| start < end && s.start as f64 - 0.5 <= start);
        if let Some(segment) = within {
            let bezier = segment
                .bezier
                .as_ref()
                .expect("a number segment has a curve");
            let origin = segment.start as f64;
            let first = solve(&bezier.x, start - origin);
            let span = solve(&bezier.x, end - origin) - first;
            for i in 0..count {
                let u = first + span * (i as f64 / last);
                let time = if i == 0 {
                    start
                } else if i + 1 == count {
                    end
                } else {
                    origin + bernstein(&bezier.x, u)
                };
                out.push((time, bernstein(&bezier.y, u)));
            }
            return true;
        }
        let mut cursor = Cursor::default();
        for i in 0..count {
            let time = if i + 1 == count && count > 1 {
                end
            } else {
                start + (end - start) * (i as f64 / last)
            };
            out.push((time, self.number_at(time, &mut cursor).0));
        }
        true
    }

    fn sample_with(&self, t: f64, cursor: &mut Cursor) -> Option<Sample> {
        let kind = self.kind?;
        if kind == Kind::Number {
            let (value, velocity) = self.number_at(t, cursor);
            return Some(Sample {
                value: Value::Number(value),
                velocity: Some(velocity),
            });
        }
        let value = match self.locate(t, cursor) {
            Located::Hold(i) => self.values[i].clone(),
            Located::In(segment) => self.values[segment.from].clone(),
        };
        Some(Sample {
            value,
            velocity: None,
        })
    }

    /// A number track's value and velocity at `t`.
    fn number_at(&self, t: f64, cursor: &mut Cursor) -> (f64, f64) {
        let number = |i: usize| self.values[i].as_number().unwrap_or(0.0);
        match self.locate(t, cursor) {
            Located::Hold(i) => (number(i), 0.0),
            Located::In(segment) => {
                let bezier = segment
                    .bezier
                    .as_ref()
                    .expect("a number segment has a curve");
                let at = t - segment.start as f64;
                let u = solve(&bezier.x, at);
                let value = bernstein(&bezier.y, u);
                let velocity = slope(bezier, u) * NANOS_PER_SECOND as f64;
                (value, velocity)
            }
        }
    }

    /// Where `t` falls: on the value the first segment starts from, at or
    /// before its start; on the value the last ends at, at or after its end;
    /// with no segment (every keypoint at one time), on the first value
    /// before that time and the last from it on; or in a segment. These are
    /// the reference results' rules (Studio's), keypoints sharing the first
    /// or last time included.
    fn locate<'s>(&'s self, t: f64, cursor: &mut Cursor) -> Located<'s> {
        let Some(first) = self.segments.first() else {
            return Located::Hold(if t < self.start as f64 {
                0
            } else {
                self.values.len().saturating_sub(1)
            });
        };
        if t <= first.start as f64 {
            return Located::Hold(first.from);
        }
        let last = self.segments.last().expect("a first segment");
        if t >= last.end as f64 {
            return Located::Hold(last.to);
        }
        // The first segment ending at or after `t`: the one an interior
        // keypoint's instant belongs to is the one that ends there.
        let fits = |s: &Segment| (s.start as f64) < t && t <= s.end as f64;
        let index = match cursor.segment {
            Some(i) if fits(&self.segments[i]) => i,
            Some(i) if i + 1 < self.segments.len() && fits(&self.segments[i + 1]) => i + 1,
            _ => self.segments.partition_point(|s| (s.end as f64) < t),
        };
        cursor.segment = Some(index);
        Located::In(&self.segments[index])
    }
}

enum Located<'s> {
    Hold(usize),
    In(&'s Segment),
}

/// The segment the previous evaluation of a batch landed in, where the next
/// time is looked for first. The parameter is solved afresh at every time,
/// so a batch and one time at a time give the same numbers.
#[derive(Default)]
struct Cursor {
    segment: Option<usize>,
}

fn bezier_of(start: &Keypoint, end: &Keypoint, cp: [Point; 2]) -> Bezier {
    let ms = NANOS_PER_MILLI as f64;
    let t0 = start.time as f64 / ms;
    let v0 = start.value.as_number().unwrap_or(0.0);
    let v3 = end.value.as_number().unwrap_or(0.0);
    Bezier {
        x: [
            0.0,
            (cp[0].t - t0) * ms,
            (cp[1].t - t0) * ms,
            end.time as f64 - start.time as f64,
        ],
        y: [v0, cp[0].v, cp[1].v, v3],
    }
}

/// The cubic Bézier with control values `p` at parameter `u`.
fn bernstein(p: &[f64; 4], u: f64) -> f64 {
    let v = 1.0 - u;
    p[0] * v * v * v + 3.0 * p[1] * v * v * u + 3.0 * p[2] * v * u * u + p[3] * u * u * u
}

fn first_derivative(p: &[f64; 4], u: f64) -> f64 {
    let v = 1.0 - u;
    3.0 * ((p[1] - p[0]) * v * v + 2.0 * (p[2] - p[1]) * v * u + (p[3] - p[2]) * u * u)
}

fn second_derivative(p: &[f64; 4], u: f64) -> f64 {
    6.0 * ((p[2] - 2.0 * p[1] + p[0]) * (1.0 - u) + (p[3] - 2.0 * p[2] + p[1]) * u)
}

fn third_derivative(p: &[f64; 4]) -> f64 {
    6.0 * (p[3] - 3.0 * p[2] + 3.0 * p[1] - p[0])
}

/// dy/dx at `u`, in value units per nanosecond. Where dx/du vanishes (a
/// collapsed handle, or a tangent vertical in time) the lowest orders of
/// parametric derivative of time and of value that do not vanish decide it:
/// of one order, their ratio is the tangent's limit; the value's of a higher
/// order, the slope is 0; the value's of a lower order, the slope is
/// unbounded, and this is its limit from earlier times (from later ones at
/// a segment's start), an infinity.
fn slope(b: &Bezier, u: f64) -> f64 {
    let dx = first_derivative(&b.x, u);
    if dx != 0.0 {
        return first_derivative(&b.y, u) / dx;
    }
    let dx = [dx, second_derivative(&b.x, u), third_derivative(&b.x)];
    let dy = [
        first_derivative(&b.y, u),
        second_derivative(&b.y, u),
        third_derivative(&b.y),
    ];
    let (Some(kx), Some(ky)) = (
        dx.iter().position(|d| *d != 0.0),
        dy.iter().position(|d| *d != 0.0),
    ) else {
        // The value does not move, or time does not (a segment of no
        // duration is never evaluated).
        return 0.0;
    };
    let ratio = dy[ky] / dx[kx];
    if ky == kx {
        ratio
    } else if ky > kx {
        0.0
    } else {
        // Near `u` the slope goes as the ratio times the parameter's gap to
        // the power ky − kx, which from earlier times is negative.
        let from_earlier = if u > 0.0 && (kx - ky) % 2 == 1 {
            -1.0
        } else {
            1.0
        };
        f64::INFINITY.copysign(ratio * from_earlier)
    }
}

/// The parameter `u` in `[0, 1]` at which the time curve `x` reaches
/// `target`, to double precision: Newton's method inside a bisection
/// bracket, from the parameter a straight line would give, stopping once a
/// step moves the parameter by no more than its last place. The result
/// depends on `x` and `target` alone.
fn solve(x: &[f64; 4], target: f64) -> f64 {
    let span = x[3];
    if target <= 0.0 {
        return 0.0;
    }
    if target >= span {
        return 1.0;
    }
    // Invariant: x(lo) < target <= x(hi).
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    let mut u = target / span;
    for _ in 0..100 {
        let error = bernstein(x, u) - target;
        if error == 0.0 {
            return u;
        }
        if error < 0.0 {
            lo = u;
        } else {
            hi = u;
        }
        let derivative = first_derivative(x, u);
        let newton = u - error / derivative;
        let next = if derivative > 0.0 && newton > lo && newton < hi {
            newton
        } else {
            0.5 * (lo + hi)
        };
        // Waiting for a step of exactly zero instead can cost dozens of
        // iterations more where rounding alternates between neighbouring
        // parameters, for no difference in the value.
        if (next - u).abs() <= f64::EPSILON * next.max(u) || hi - lo <= f64::EPSILON * hi {
            return next;
        }
        u = next;
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::{Easing, Transition, Transitions};

    fn keypoint(time: Nanos, value: Value) -> Keypoint {
        Keypoint {
            id: format!("k{time}"),
            time,
            value,
            transitions: Transitions::default(),
        }
    }

    fn track(points: Vec<Keypoint>) -> Track {
        Track {
            id: "t".into(),
            target: "x".into(),
            keypoints: points,
            ..Track::default()
        }
    }

    const S: Nanos = NANOS_PER_SECOND;

    #[test]
    fn a_linear_segment_plays_a_line() {
        let mut points = vec![
            keypoint(0, Value::Number(0.0)),
            keypoint(S, Value::Number(10.0)),
        ];
        points[0].transitions.leave = Some(Transition::Easing(Easing::Linear));
        points[1].transitions.arrive = Some(Transition::Easing(Easing::Linear));
        let curve = Curve::new(&track(points));
        for i in 0..=100 {
            let t = i as f64 * 1e7;
            let sample = curve.sample(t).unwrap();
            let value = sample.value.as_number().unwrap();
            assert!((value - t / 1e8).abs() < 1e-12, "{t}: {value}");
            if i > 0 && i < 100 {
                assert!((sample.velocity.unwrap() - 10.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn holds_outside_its_keypoints_with_no_velocity() {
        let curve = Curve::new(&track(vec![
            keypoint(-S, Value::Number(-1.0)),
            keypoint(S, Value::Number(3.0)),
        ]));
        let at = |t: f64| curve.sample(t).unwrap();
        assert_eq!(at(-2e9).value, Value::Number(-1.0));
        assert_eq!(at(-1e9).velocity, Some(0.0));
        assert_eq!(at(1e9).value, Value::Number(3.0));
        assert_eq!(at(5e9).velocity, Some(0.0));
        // Before 0 is a time like any other.
        assert!(at(-0.5e9).value.as_number().unwrap() > -1.0);
    }

    #[test]
    fn a_step_holds_through_an_interior_keypoint_and_plays_the_last_at_its_instant() {
        let curve = Curve::new(&track(vec![
            keypoint(0, Value::Bool(false)),
            keypoint(S, Value::Bool(true)),
            keypoint(2 * S, Value::Bool(false)),
        ]));
        let at = |t: f64| curve.sample(t).unwrap().value;
        assert_eq!(at(0.999e9), Value::Bool(false));
        assert_eq!(at(1e9), Value::Bool(false));
        assert_eq!(at(1e9 + 1.0), Value::Bool(true));
        assert_eq!(at(2e9 - 1.0), Value::Bool(true));
        assert_eq!(at(2e9), Value::Bool(false));
        assert_eq!(curve.sample(1.5e9).unwrap().velocity, None);
    }

    #[test]
    fn keypoints_sharing_a_time_jump_to_the_later_one() {
        let curve = Curve::new(&track(vec![
            keypoint(0, Value::Text("a".into())),
            keypoint(S, Value::Text("b".into())),
            keypoint(S, Value::Text("c".into())),
            keypoint(2 * S, Value::Text("d".into())),
        ]));
        let at = |t: f64| curve.sample(t).unwrap().value;
        assert_eq!(at(1e9), Value::Text("a".into()));
        assert_eq!(at(1.5e9), Value::Text("c".into()));
        let single = Curve::new(&track(vec![keypoint(5, Value::Number(2.0))]));
        assert_eq!(single.sample(-1e12).unwrap().value, Value::Number(2.0));
        assert_eq!(Curve::new(&track(vec![])).sample(0.0), None);
    }

    #[test]
    fn keypoints_sharing_the_first_or_last_time_hold_what_the_segments_start_and_end_at() {
        let text = |s: &str| Value::Text(s.into());
        let ends = Curve::new(&track(vec![
            keypoint(0, text("a")),
            keypoint(0, text("b")),
            keypoint(S, text("c")),
            keypoint(S, text("d")),
        ]));
        let at = |t: f64| ends.sample(t).unwrap().value;
        assert_eq!(at(-1.0), text("b"));
        assert_eq!(at(0.0), text("b"));
        assert_eq!(at(0.5e9), text("b"));
        assert_eq!(at(1e9), text("c"));
        assert_eq!(at(5e9), text("c"));
        // Every keypoint at one time: the first before it, the last from it on.
        let one_time = Curve::new(&track(vec![
            keypoint(-1_000_000, Value::Number(1.0)),
            keypoint(-1_000_000, Value::Number(2.0)),
        ]));
        let values: Vec<Value> = [-2e6, -1e6, 0.0]
            .iter()
            .map(|t| one_time.sample(*t).unwrap().value)
            .collect();
        assert_eq!(values, [1.0, 2.0, 2.0].map(Value::Number).to_vec());
        let numbers = Curve::new(&track(vec![
            keypoint(0, Value::Number(0.0)),
            keypoint(S, Value::Number(1.0)),
            keypoint(S, Value::Number(7.0)),
        ]));
        assert_eq!(numbers.sample(1e9).unwrap().value, Value::Number(1.0));
        assert_eq!(numbers.sample(2e9).unwrap().value, Value::Number(1.0));
    }

    #[test]
    fn a_tangent_vertical_in_time_has_an_unbounded_velocity() {
        // Time stalls at the middle of the segment while the value rises
        // through it.
        let mut points = vec![
            keypoint(0, Value::Number(0.0)),
            keypoint(S, Value::Number(1.0)),
        ];
        points[0].transitions.leave = Some(Transition::Handle { dt: 1e9, dv: 0.0 });
        points[1].transitions.arrive = Some(Transition::Handle { dt: -1e9, dv: 0.0 });
        let curve = Curve::new(&track(points));
        let velocity = |t: f64| curve.sample(t).unwrap().velocity.unwrap();
        assert_eq!(velocity(0.5e9), f64::INFINITY);
        assert!(velocity(0.5e9 - 1.0) > 1e5 && velocity(0.5e9 + 1.0) > 1e5);
        // An interior keypoint arrived at by a handle of no duration and
        // some height: the slope from earlier times, rising.
        let mut points = vec![
            keypoint(0, Value::Number(0.0)),
            keypoint(S, Value::Number(1.0)),
            keypoint(2 * S, Value::Number(0.0)),
        ];
        points[0].transitions.leave = Some(Transition::Easing(Easing::Linear));
        points[1].transitions.arrive = Some(Transition::Handle { dt: 0.0, dv: -0.5 });
        let curve = Curve::new(&track(points));
        assert_eq!(curve.sample(1e9).unwrap().velocity, Some(f64::INFINITY));
        assert!(curve.sample(1e9 - 1e3).unwrap().velocity.unwrap() > 0.0);
    }

    #[test]
    fn a_batch_equals_one_time_at_a_time_in_any_order() {
        let mut points = vec![
            keypoint(0, Value::Number(0.0)),
            keypoint(S / 3, Value::Number(5.0)),
            keypoint(S, Value::Number(-2.0)),
            keypoint(2 * S, Value::Number(1.0)),
        ];
        points[1].transitions.arrive = Some(Transition::Auto);
        points[1].transitions.leave = Some(Transition::AutoClamped);
        points[2].transitions.leave = Some(Transition::Handle { dt: 2e8, dv: 3.0 });
        let curve = Curve::new(&track(points));
        let mut times: Vec<f64> = (-10..=250).map(|i| i as f64 * 1e7 + 3.0).collect();
        let mut forward = Vec::new();
        curve.sample_many(&times, &mut forward);
        times.reverse();
        let mut backward = Vec::new();
        curve.sample_many(&times, &mut backward);
        backward.reverse();
        times.reverse();
        for (i, t) in times.iter().enumerate() {
            let one = curve.sample(*t).unwrap();
            assert_eq!(forward[i], one, "at {t}");
            assert_eq!(backward[i], one, "at {t}");
        }
        let mut values = vec![0.0; times.len()];
        let mut velocities = vec![0.0; times.len()];
        assert!(curve.sample_numbers(&times, &mut values, &mut velocities));
        for (i, sample) in forward.iter().enumerate() {
            assert_eq!(sample.value, Value::Number(values[i]));
            assert_eq!(sample.velocity, Some(velocities[i]));
        }
    }

    #[test]
    fn a_batch_at_the_keypoints_own_instants_keeps_the_step_rule() {
        let curve = Curve::new(&track(vec![
            keypoint(0, Value::Bool(false)),
            keypoint(S, Value::Bool(true)),
            keypoint(2 * S, Value::Bool(false)),
            keypoint(3 * S, Value::Bool(true)),
        ]));
        let times: Vec<f64> = (0..=12).map(|i| i as f64 * 0.25e9).collect();
        let mut batch = Vec::new();
        curve.sample_many(&times, &mut batch);
        for (t, sample) in times.iter().zip(&batch) {
            assert_eq!(*sample, curve.sample(*t).unwrap(), "at {t}");
        }
        assert_eq!(
            batch[4].value,
            Value::Bool(false),
            "an interior keypoint's instant holds"
        );
        assert_eq!(batch[8].value, Value::Bool(true));
    }

    #[test]
    fn a_collapsed_handle_gives_the_limit_of_the_slope() {
        // The segment arrives at its end with its handle on the keypoint, so
        // the time curve stalls there: the velocity at the instant is the
        // slope's limit, which a finite difference approaches.
        let mut points = vec![
            keypoint(0, Value::Number(0.0)),
            keypoint(S, Value::Number(10.0)),
            keypoint(2 * S, Value::Number(0.0)),
        ];
        points[0].transitions.leave = Some(Transition::Handle { dt: 4e8, dv: 0.0 });
        points[1].transitions.arrive = Some(Transition::Handle { dt: 0.0, dv: 0.0 });
        let curve = Curve::new(&track(points));
        let at = curve.sample(1e9).unwrap().velocity.unwrap();
        let near = |t: f64| curve.sample(t).unwrap().value.as_number().unwrap();
        let difference = (near(1e9) - near(1e9 - 1e3)) / 1e-6;
        assert!(at.is_finite() && at != 0.0, "{at}");
        assert!(
            (at - difference).abs() < 1e-3 * at.abs(),
            "{at} vs {difference}"
        );
    }

    #[test]
    fn a_polyline_lies_on_the_curve_and_spaces_a_segment_by_its_parameter() {
        let mut points = vec![
            keypoint(-S, Value::Number(-1.0)),
            keypoint(S, Value::Number(4.0)),
            keypoint(2 * S, Value::Number(0.5)),
        ];
        points[0].transitions.leave = Some(Transition::Easing(Easing::Expo));
        points[1].transitions.arrive = Some(Transition::Handle {
            dt: -0.9e9,
            dv: 3.0,
        });
        points[1].transitions.leave = Some(Transition::Easing(Easing::Back));
        let curve = Curve::new(&track(points));
        let ranges = [
            // A whole segment, part of one, and one ending at a keypoint.
            (-1e9, 1e9),
            (-0.7e9, 0.2e9),
            (1.3e9, 2e9),
            // A hold before the first keypoint, after the last, and a range
            // across a keypoint.
            (-3e9, -1e9),
            (2e9, 5e9),
            (0.5e9, 1.5e9),
        ];
        for (start, end) in ranges {
            let mut out = Vec::new();
            assert!(curve.polyline(start, end, 51, &mut out));
            assert_eq!(out.len(), 51);
            assert_eq!(out[0].0, start);
            assert_eq!(out[50].0, end);
            for pair in out.windows(2) {
                assert!(pair[0].0 <= pair[1].0, "{start}..{end}: {pair:?}");
            }
            for &(time, value) in &out {
                let expected = curve.sample(time).unwrap().value.as_number().unwrap();
                assert!(
                    (value - expected).abs() <= 1e-12 * expected.abs().max(1.0),
                    "{start}..{end} at {time}: {value} vs {expected}"
                );
            }
        }
        // Within a segment the points are evenly spaced in its parameter.
        let segment = curve.played_segments()[0];
        let mut out = Vec::new();
        curve.polyline(-1e9, 1e9, 3, &mut out);
        let x: Vec<f64> = segment.control_points.iter().map(|p| p.0).collect();
        let middle = bernstein(&[x[0], x[1], x[2], x[3]], 0.5);
        assert!(
            (out[1].0 - middle).abs() <= 1e-6,
            "{} vs {middle}",
            out[1].0
        );
        // Elsewhere, evenly in time.
        out.clear();
        curve.polyline(0.5e9, 1.5e9, 3, &mut out);
        assert_eq!(out[1].0, 1e9);
        // A segment's ends as an editor holds them, off by the rounding to
        // whole nanoseconds, still draw it by its parameter.
        for (start, end) in [(-1e9 - 0.4, 1e9 + 0.4), (-1e9 + 0.4, 1e9 - 0.4)] {
            out.clear();
            curve.polyline(start, end, 3, &mut out);
            assert!((out[1].0 - middle).abs() <= 1.0, "{} vs {middle}", out[1].0);
        }
    }

    #[test]
    fn a_polyline_draws_only_a_number_track() {
        let curve = Curve::new(&track(vec![
            keypoint(0, Value::Bool(false)),
            keypoint(S, Value::Bool(true)),
        ]));
        let mut out = Vec::new();
        assert!(!curve.polyline(0.0, 1e9, 5, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn the_solver_finds_the_parameter_to_double_precision() {
        // A curve whose time stalls at its start (a collapsed handle), one
        // that stalls in its middle (a vertical tangent), and one nearly
        // linear.
        for x in [
            [0.0, 0.0, 0.6e9, 1e9],
            [0.0, 0.33e9, 0.66e9, 1e9],
            [0.0, 1e9, 0.0, 1e9],
        ] {
            for i in 1..1000 {
                let target = i as f64 * 1e6;
                let u = solve(&x, target);
                let residual = (bernstein(&x, u) - target).abs();
                assert!(
                    residual <= 1e-6,
                    "{x:?} at {target}: x({u}) is {residual} off"
                );
            }
        }
    }
}
