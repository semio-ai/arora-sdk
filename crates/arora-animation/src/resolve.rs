//! Handle resolution: a number segment's two Bézier control points, from the
//! transitions its keypoints' sides carry.
//!
//! These are the rules Semio Studio's editor draws a curve with
//! (`resolveSegmentControlPoints` in `@semio/animation`), so a clip plays the
//! curve its author saw. Resolution has two layers:
//!
//! - **raw**: the authored direction. An explicit handle is its own delta;
//!   an easing places its control point on the segment by the easing's
//!   parameters; an inferred tangent comes from the neighbouring keypoints
//!   ([`infer_auto`]), flattened at an extremum and held inside its segment
//!   when it is the clamped one.
//! - **effective**: the raw handles passed through clamps that only scale a
//!   handle along its own ray: no handle reaches outside its segment in time,
//!   the segment's time stays monotonic (no knot), and a clamped inferred
//!   tangent stays within the segment's value range.
//!
//! **Resolution works in milliseconds**, as the editor does. The inferred
//! tangent normalizes a (time, value) direction, and the clamps compare
//! against tolerances in those units, so the time unit is part of the rule:
//! resolved in nanoseconds, the same clip would play another curve.

use crate::clip::{Easing, Transition};
use crate::time::NANOS_PER_MILLI;

/// A point of a segment's plane: `t` in milliseconds of clip-local time, `v`
/// in value units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub t: f64,
    pub v: f64,
}

impl Point {
    pub fn new(t: f64, v: f64) -> Point {
        Point { t, v }
    }
}

/// A number segment to resolve, and the keypoints either side of it that an
/// inferred tangent reads.
#[derive(Clone, Copy, Debug)]
pub struct SegmentInput {
    pub start: Point,
    pub end: Point,
    /// The start keypoint's leaving side; absent, [`Transition::DEFAULT`].
    pub leave: Option<Transition>,
    /// The end keypoint's arriving side; absent, [`Transition::DEFAULT`].
    pub arrive: Option<Transition>,
    /// The keypoint before `start`, in time order.
    pub before: Option<Point>,
    /// The keypoint after `end`, in time order.
    pub after: Option<Point>,
}

/// A segment's control points, raw and effective (see the module's
/// documentation). A curve plays the effective ones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolved {
    pub raw: [Point; 2],
    pub effective: [Point; 2],
}

/// A raw handle: a delta from its anchor, or an easing still to be placed on
/// the segment.
#[derive(Clone, Copy, Debug)]
enum Raw {
    Delta(Point),
    Easing(Easing),
}

/// Which side of a segment a handle is on: the start keypoint's leaving side
/// or the end keypoint's arriving side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Leave,
    Arrive,
}

const EPSILON: f64 = 1e-6;
const AUTO_MAGIC_SCALE: f64 = 2.5614;
const ASYMMETRY_CLAMP_FACTOR: f64 = 5.0;

/// The standard easings' control points, as CSS-style timing parameters in
/// segment space: the leaving side's.
fn ease_out(easing: Easing) -> Point {
    let (x, y) = match easing {
        Easing::Sine => (0.37, 0.0),
        Easing::Cubic => (0.65, 0.0),
        Easing::Quint => (0.83, 0.0),
        Easing::Circ => (0.85, 0.0),
        Easing::Quad => (0.45, 0.0),
        Easing::Quart => (0.76, 0.0),
        Easing::Expo => (0.87, 0.0),
        Easing::Back => (0.68, -0.6),
        Easing::Linear => (0.33, 0.33),
    };
    Point::new(x, y)
}

/// The arriving side's parameters; see [`ease_out`].
fn ease_in(easing: Easing) -> Point {
    let (x, y) = match easing {
        Easing::Sine => (0.63, 1.0),
        Easing::Cubic => (0.35, 1.0),
        Easing::Quint => (0.17, 1.0),
        Easing::Circ => (0.15, 1.0),
        Easing::Quad => (0.55, 1.0),
        Easing::Quart => (0.24, 1.0),
        Easing::Expo => (0.13, 1.0),
        Easing::Back => (0.32, 1.6),
        Easing::Linear => (0.66, 0.66),
    };
    Point::new(x, y)
}

/// Resolves a number segment's control points.
pub fn resolve_segment(input: &SegmentInput) -> Resolved {
    let leave = input.leave.unwrap_or(Transition::DEFAULT);
    let arrive = input.arrive.unwrap_or(Transition::DEFAULT);
    let raw_leave = raw_side(Side::Leave, leave, input);
    let raw_arrive = raw_side(Side::Arrive, arrive, input);
    let (start, end) = (input.start, input.end);

    let cp1_raw = add(start, delta_of(Side::Leave, raw_leave, start, end));
    let cp2_raw = add(end, delta_of(Side::Arrive, raw_arrive, start, end));
    let (mut cp1, mut cp2) = monotonic(start, end, cp1_raw, cp2_raw);

    // A clamped inferred tangent also stays inside its segment's value range
    // (and so its time range): the extremum flatten is in its raw direction,
    // and these clamps scale it along that direction only.
    let (min_t, max_t) = (start.t.min(end.t), start.t.max(end.t));
    if leave == Transition::AutoClamped {
        let clamped = within_times(start, within_values(start, end, cp1), min_t, max_t);
        if !points_equal(clamped, cp1) {
            cp1 = clamped;
        }
    }
    if arrive == Transition::AutoClamped {
        let clamped = within_times(end, within_values(end, start, cp2), min_t, max_t);
        if !points_equal(clamped, cp2) {
            cp2 = clamped;
        }
    }
    Resolved {
        raw: [cp1_raw, cp2_raw],
        effective: [cp1, cp2],
    }
}

/// A side's raw handle: an explicit handle in milliseconds, an easing, or
/// the inferred tangent (the default easing where nothing can be inferred).
fn raw_side(side: Side, transition: Transition, input: &SegmentInput) -> Raw {
    match transition {
        Transition::Handle { dt, dv } => Raw::Delta(Point::new(dt / NANOS_PER_MILLI as f64, dv)),
        Transition::Easing(easing) => Raw::Easing(easing),
        Transition::Auto | Transition::AutoClamped => {
            let clamped = transition == Transition::AutoClamped;
            let (before, anchor, after) = match side {
                Side::Leave => (input.before, input.start, Some(input.end)),
                Side::Arrive => (Some(input.start), input.end, input.after),
            };
            let inferred = if clamped {
                infer_auto_clamped(before, anchor, after)
            } else {
                infer_auto(before, anchor, after)
            };
            let Some([arriving, leaving]) = inferred else {
                return Raw::Easing(Easing::Cubic);
            };
            let delta = match side {
                Side::Leave => leaving,
                Side::Arrive => arriving,
            };
            if clamped {
                Raw::Delta(clamp_inferred_to_segment(
                    side,
                    input.start,
                    input.end,
                    delta,
                ))
            } else {
                Raw::Delta(delta)
            }
        }
    }
}

/// A raw handle as a delta from its anchor: the start keypoint for the
/// leaving side, the end keypoint for the arriving one.
fn delta_of(side: Side, raw: Raw, start: Point, end: Point) -> Point {
    match raw {
        Raw::Delta(delta) => delta,
        Raw::Easing(easing) => {
            let params = match side {
                Side::Leave => ease_out(easing),
                Side::Arrive => ease_in(easing),
            };
            let t = params.t * (end.t - start.t) + start.t;
            let v = start.v + (end.v - start.v) * params.v;
            let anchor = match side {
                Side::Leave => start,
                Side::Arrive => end,
            };
            Point::new(t - anchor.t, v - anchor.v)
        }
    }
}

/// The effective handles of a segment of non-zero duration: neither reaches
/// outside the segment in time, and the segment's time is monotonic in the
/// curve's parameter. Each clamp scales a handle along its ray from its
/// anchor.
fn monotonic(start: Point, end: Point, cp1: Point, cp2: Point) -> (Point, Point) {
    let span = end.t - start.t;
    if span == 0.0 {
        return (to_time(start, cp1, start.t), to_time(end, cp2, end.t));
    }
    let mut cp1 = cp1;
    let mut cp2 = cp2;
    if cp1.t < start.t {
        cp1 = to_time(start, cp1, start.t);
    }
    if cp2.t > end.t {
        cp2 = to_time(end, cp2, end.t);
    }
    let n1 = (cp1.t - start.t) / span;
    let n2 = (cp2.t - start.t) / span;
    if is_monotonic(n1, n2) {
        return (cp1, cp2);
    }
    let (m1, m2) = monotonic_handles(n1, n2);
    let target1 = to_time(start, cp1, start.t + m1 * span);
    let target2 = to_time(end, cp2, start.t + m2 * span);
    if !points_equal(cp1, target1) {
        cp1 = target1;
    }
    if !points_equal(cp2, target2) {
        cp2 = target2;
    }
    (cp1, cp2)
}

/// Whether a timing curve with normalized control-point times `x1` and `x2`
/// never runs backwards, within [`EPSILON`].
fn is_monotonic(x1: f64, x2: f64) -> bool {
    derivative_minimum(x1, x2) >= -EPSILON
}

/// The minimum over `[0, 1]` of the derivative of the normalized timing
/// curve `x(s)` with control-point times `x1` and `x2`.
fn derivative_minimum(x1: f64, x2: f64) -> f64 {
    let a = 1.0 + 3.0 * (x1 - x2);
    let b = 3.0 * (x2 - 2.0 * x1);
    let c = 3.0 * x1;
    let (qa, qb, qc) = (3.0 * a, 2.0 * b, c);
    let derivative = |t: f64| qa * t * t + qb * t + qc;
    let mut min = derivative(0.0).min(derivative(1.0));
    if qa.abs() > EPSILON {
        let t = -qb / (2.0 * qa);
        if (0.0..=1.0).contains(&t) {
            min = min.min(derivative(t));
        }
    }
    min
}

/// The longest shortening of both handles towards their anchors, by one
/// factor, that makes the timing monotonic: a bisection over the factor.
fn monotonic_handles(x1: f64, x2: f64) -> (f64, f64) {
    let shortened = |f: f64| (x1 * f, 1.0 + (x2 - 1.0) * f);
    let (mut low, mut high) = (0.0, 1.0);
    let mut best = None;
    for _ in 0..30 {
        let mid = (low + high) / 2.0;
        let (c1, c2) = shortened(mid);
        if is_monotonic(c1, c2) {
            best = Some((c1, c2));
            low = mid;
        } else {
            high = mid;
        }
    }
    best.unwrap_or((0.0, 1.0))
}

/// The automatic tangent at `anchor` between `before` and `after`, as the
/// deltas of its arriving and leaving handles: the bisector of the two
/// (time, value) directions to the neighbours, each handle a share of the
/// distance to its neighbour (that distance held to five times the other
/// side's). `None` at the first or last keypoint, or where the directions
/// cancel.
pub fn infer_auto(
    before: Option<Point>,
    anchor: Point,
    after: Option<Point>,
) -> Option<[Point; 2]> {
    let (before, after) = (before?, after?);
    let (to_before, before_length) = unit(Point::new(anchor.t - before.t, anchor.v - before.v));
    let (to_after, after_length) = unit(Point::new(after.t - anchor.t, after.v - anchor.v));
    let direction = Point::new(to_before.t + to_after.t, to_before.v + to_after.v);
    let scale = direction.t.hypot(direction.v) * AUTO_MAGIC_SCALE;
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let arriving = before_length.min(ASYMMETRY_CLAMP_FACTOR * after_length) / scale;
    let leaving = after_length.min(ASYMMETRY_CLAMP_FACTOR * before_length) / scale;
    Some([
        Point::new(-direction.t * arriving, -direction.v * arriving),
        Point::new(direction.t * leaving, direction.v * leaving),
    ])
}

/// [`infer_auto`], flattened to horizontal where `anchor` is a local
/// extremum or equals a neighbour, so the curve does not overshoot it. The
/// handles keep their time, so both stay on one line through the anchor.
pub fn infer_auto_clamped(
    before: Option<Point>,
    anchor: Point,
    after: Option<Point>,
) -> Option<[Point; 2]> {
    let [arriving, leaving] = infer_auto(before, anchor, after)?;
    let (before, after) = (before?, after?);
    if is_extremum(before, anchor, after) {
        return Some([Point::new(arriving.t, 0.0), Point::new(leaving.t, 0.0)]);
    }
    Some([arriving, leaving])
}

fn is_extremum(before: Point, anchor: Point, after: Point) -> bool {
    let rise_in = anchor.v - before.v;
    let rise_out = after.v - anchor.v;
    if rise_in.abs() < EPSILON || rise_out.abs() < EPSILON {
        return true;
    }
    js_sign(rise_in) != js_sign(rise_out)
}

/// `Math.sign`: −1, 0 or 1.
fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// A unit vector along `delta` and `delta`'s length; a zero or non-finite
/// delta has no direction and counts as length 1.
fn unit(delta: Point) -> (Point, f64) {
    let length = delta.t.hypot(delta.v);
    if !length.is_finite() || length == 0.0 {
        return (Point::new(0.0, 0.0), 1.0);
    }
    (Point::new(delta.t / length, delta.v / length), length)
}

/// A clamped inferred tangent's raw delta, kept inside its segment in time
/// and in value.
fn clamp_inferred_to_segment(side: Side, start: Point, end: Point, delta: Point) -> Point {
    let (anchor, opposite) = match side {
        Side::Leave => (start, end),
        Side::Arrive => (end, start),
    };
    let (min_t, max_t) = (start.t.min(end.t), start.t.max(end.t));
    let mut cp = add(anchor, delta);
    cp = within_times(anchor, cp, min_t, max_t);
    cp = within_values(anchor, opposite, cp);
    // Holding the value can move the time: hold the time again.
    cp = within_times(anchor, cp, min_t, max_t);
    Point::new(cp.t - anchor.t, cp.v - anchor.v)
}

/// `cp` scaled along its ray from `anchor` into the value range between
/// `anchor` and `opposite`.
fn within_values(anchor: Point, opposite: Point, cp: Point) -> Point {
    let (min_v, max_v) = (anchor.v.min(opposite.v), anchor.v.max(opposite.v));
    if cp.v >= min_v && cp.v <= max_v {
        return cp;
    }
    let target = if cp.v < min_v { min_v } else { max_v };
    // Held to the anchor's own value (a flat tangent), the handle keeps its
    // time rather than collapsing onto the anchor.
    if (target - anchor.v).abs() < EPSILON {
        return Point::new(cp.t, target);
    }
    let dv = cp.v - anchor.v;
    if dv == 0.0 {
        return Point::new(anchor.t, target);
    }
    let ratio = (target - anchor.v) / dv;
    if !ratio.is_finite() {
        return Point::new(anchor.t, target);
    }
    Point::new(anchor.t + (cp.t - anchor.t) * ratio, anchor.v + dv * ratio)
}

/// `cp` scaled along its ray from `anchor` into `[min_t, max_t]`.
fn within_times(anchor: Point, cp: Point, min_t: f64, max_t: f64) -> Point {
    if cp.t >= min_t && cp.t <= max_t {
        return cp;
    }
    let target = if cp.t < min_t { min_t } else { max_t };
    let dt = cp.t - anchor.t;
    if dt == 0.0 {
        return Point::new(target, anchor.v);
    }
    let ratio = (target - anchor.t) / dt;
    if !ratio.is_finite() {
        return Point::new(target, anchor.v);
    }
    Point::new(anchor.t + dt * ratio, anchor.v + (cp.v - anchor.v) * ratio)
}

/// `cp` scaled along its ray from `anchor` to the time `target`, unless it
/// is already within [`EPSILON`] of it.
fn to_time(anchor: Point, cp: Point, target: f64) -> Point {
    if (target - cp.t).abs() < EPSILON {
        return cp;
    }
    let dt = cp.t - anchor.t;
    if dt == 0.0 {
        return Point::new(target, anchor.v);
    }
    let ratio = (target - anchor.t) / dt;
    if !ratio.is_finite() {
        return Point::new(target, anchor.v);
    }
    Point::new(anchor.t + dt * ratio, anchor.v + (cp.v - anchor.v) * ratio)
}

fn add(anchor: Point, delta: Point) -> Point {
    Point::new(anchor.t + delta.t, anchor.v + delta.v)
}

fn points_equal(a: Point, b: Point) -> bool {
    (a.t - b.t).abs() < EPSILON && (a.v - b.v).abs() < EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(leave: Option<Transition>, arrive: Option<Transition>) -> SegmentInput {
        SegmentInput {
            start: Point::new(0.0, 0.0),
            end: Point::new(1000.0, 10.0),
            leave,
            arrive,
            before: None,
            after: None,
        }
    }

    #[test]
    fn an_absent_side_eases_as_a_cubic() {
        let resolved = resolve_segment(&segment(None, None));
        assert_eq!(
            resolved.effective,
            [Point::new(650.0, 0.0), Point::new(350.0, 10.0)]
        );
    }

    #[test]
    fn an_explicit_handle_reaching_past_its_segment_is_scaled_back_along_its_ray() {
        let handle = |dt_ms: f64, dv: f64| Transition::Handle {
            dt: dt_ms * 1e6,
            dv,
        };
        let resolved = resolve_segment(&segment(
            Some(handle(-200.0, 4.0)),
            Some(handle(300.0, 3.0)),
        ));
        // Both point the wrong way in time: each is scaled to its anchor's time.
        assert_eq!(resolved.effective[0], Point::new(0.0, 0.0));
        assert_eq!(resolved.effective[1], Point::new(1000.0, 10.0));
        // Both reach past the other end: shortened together until monotonic.
        let resolved = resolve_segment(&segment(
            Some(handle(1500.0, 0.0)),
            Some(handle(-1500.0, 0.0)),
        ));
        let [cp1, cp2] = resolved.effective;
        // Shortened to where the timing stops running backwards, within the
        // tolerance the editor's check allows.
        assert!(cp1.t > 0.0 && cp1.t <= 1000.001, "{cp1:?}");
        assert!(cp2.t >= -0.001 && cp2.t < 1000.0, "{cp2:?}");
        assert!(is_monotonic(cp1.t / 1000.0, cp2.t / 1000.0));
    }

    /// Control points Studio's editor resolves (`resolveSegmentControlPoints`
    /// in `@semio/animation`) for clamps the reference set does not reach: an
    /// inferred clamped tangent past its segment's value range, on either
    /// side; explicit handles near a knot and pointing back in time; an
    /// unclamped tangent between steep neighbours.
    #[test]
    fn the_clamps_place_the_points_studios_editor_does() {
        let p = Point::new;
        let ms = |x: f64, y: f64| Transition::Handle { dt: x * 1e6, dv: y };
        let cases = [
            (
                SegmentInput {
                    start: p(10.0, 100.0),
                    end: p(1000.0, 101.0),
                    leave: Some(Transition::AutoClamped),
                    arrive: Some(Transition::Easing(Easing::Cubic)),
                    before: Some(p(0.0, 0.0)),
                    after: Some(p(2000.0, 150.0)),
                },
                [p(11.103866472134445, 101.0), p(356.5, 101.0)],
            ),
            (
                SegmentInput {
                    start: p(0.0, 0.0),
                    end: p(990.0, 1.0),
                    leave: Some(Transition::Easing(Easing::Linear)),
                    arrive: Some(Transition::AutoClamped),
                    before: Some(p(-500.0, -3.0)),
                    after: Some(p(1000.0, 101.0)),
                },
                [p(326.7, 0.33), p(988.8961335278656, 0.0)],
            ),
            (
                SegmentInput {
                    start: p(0.0, 0.0),
                    end: p(1000.0, 10.0),
                    leave: Some(ms(900.0, 30.0)),
                    arrive: Some(ms(-950.0, -20.0)),
                    before: None,
                    after: None,
                },
                [p(900.0, 30.0), p(50.0, -10.0)],
            ),
            (
                SegmentInput {
                    start: p(0.0, 0.0),
                    end: p(1000.0, 10.0),
                    leave: Some(ms(-300.0, 6.0)),
                    arrive: Some(Transition::Easing(Easing::Quint)),
                    before: None,
                    after: None,
                },
                [p(0.0, 0.0), p(170.0, 10.0)],
            ),
            (
                SegmentInput {
                    start: p(10.0, 100.0),
                    end: p(1000.0, 101.0),
                    leave: Some(Transition::Auto),
                    arrive: Some(Transition::Auto),
                    before: Some(p(0.0, 0.0)),
                    after: Some(p(1010.0, 300.0)),
                },
                [
                    p(155.39117228714107, 231.71083274774304),
                    p(720.0581530804511, -165.49703302973631),
                ],
            ),
        ];
        // The raw points too, which the editor draws its markers at: they
        // differ from the effective ones only where a clamp shortened a
        // handle along its ray.
        let raw = [
            [p(11.103866472134445, 101.0), p(356.5, 101.0)],
            [p(326.7, 0.33), p(988.8961335278656, 0.0)],
            [p(900.0, 30.0), p(50.0, -10.0)],
            [p(-300.0, 6.0), p(170.0, 10.0)],
            cases[4].1,
        ];
        for (i, (input, expected)) in cases.iter().enumerate() {
            let resolved = resolve_segment(input);
            for (got, expected) in [(resolved.effective, expected), (resolved.raw, &raw[i])] {
                for (g, e) in got.iter().zip(expected) {
                    assert!(
                        (g.t - e.t).abs() < 1e-9 && (g.v - e.v).abs() < 1e-9,
                        "case {i}: {got:?}, Studio resolves {expected:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_inferred_tangent_reads_the_neighbours() {
        let mut input = segment(Some(Transition::Auto), Some(Transition::AutoClamped));
        input.before = Some(Point::new(-1000.0, -10.0));
        input.after = Some(Point::new(2000.0, 0.0));
        let resolved = resolve_segment(&input);
        // Leaving a straight line, the tangent follows it.
        let [cp1, _] = resolved.effective;
        assert!((cp1.v / cp1.t - 0.01).abs() < 1e-12, "{cp1:?}");
        // Arriving at a peak, the clamped tangent is flat.
        assert_eq!(resolved.effective[1].v, 10.0);
        // With no neighbour to read, the tangent is the default easing.
        let resolved = resolve_segment(&segment(Some(Transition::Auto), None));
        assert_eq!(resolved.effective[0], Point::new(650.0, 0.0));
    }
}
