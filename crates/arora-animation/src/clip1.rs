//! Clip format 1.x, read into format 2.0.
//!
//! A 1.x clip has a duration in milliseconds and keypoints at normalized
//! stamps (`0` to `1` of the duration), each side of a number segment a
//! CSS-style timing handle normalized to the segment: `x` its share of the
//! segment's time, `y` of its change in value; a side with no handle eases
//! `(0.42, 0)` leaving and `(0.58, 1)` arriving.
//!
//! The reading is exact: a normalized handle is the explicit handle of 2.0
//! at `(x × Δt, y × Δv)` from its segment's start, and a cubic Bézier is
//! unchanged by that affine map. The keypoints' stamps become whole
//! nanoseconds of the duration. A 1.x track whose numbers are all
//! single-precision keeps outputting them in single precision.

use crate::clip::{Clip, Keypoint, Track, Transition, Transitions};
use crate::time::{Nanos, NANOS_PER_MILLI};
use crate::value::Value;

/// A 1.x clip.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipV1 {
    pub name: String,
    /// The duration the stamps are fractions of, in milliseconds.
    pub duration_ms: u32,
    pub tracks: Vec<TrackV1>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackV1 {
    pub id: String,
    pub name: String,
    pub animatable_id: String,
    pub points: Vec<KeypointV1>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeypointV1 {
    pub id: String,
    /// The keypoint's time as a fraction of the clip's duration.
    pub stamp: f32,
    pub value: ValueV1,
    /// The arriving handle, normalized to the segment that ends here.
    pub arrive: Option<(f32, f32)>,
    /// The leaving handle, normalized to the segment that starts here.
    pub leave: Option<(f32, f32)>,
}

/// A 1.x keypoint value: a number of either width, a boolean or a string.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueV1 {
    F32(f32),
    F64(f64),
    Bool(bool),
    Text(String),
}

impl ValueV1 {
    fn value(&self) -> Value {
        match self {
            ValueV1::F32(v) => Value::Number(f64::from(*v)),
            ValueV1::F64(v) => Value::Number(*v),
            ValueV1::Bool(v) => Value::Bool(*v),
            ValueV1::Text(v) => Value::Text(v.clone()),
        }
    }
}

const DEFAULT_LEAVE: (f32, f32) = (0.42, 0.0);
const DEFAULT_ARRIVE: (f32, f32) = (0.58, 1.0);

/// Why a 1.x clip cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipV1Error {
    /// A stamp or a handle that is not finite.
    NotFinite { track: String, keypoint: String },
}

impl std::fmt::Display for ClipV1Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipV1Error::NotFinite { track, keypoint } => write!(
                f,
                "keypoint {keypoint:?} of track {track:?} has a stamp or a handle that is not finite"
            ),
        }
    }
}

impl std::error::Error for ClipV1Error {}

impl ClipV1 {
    /// The clip in format 2.0, and which of its tracks output
    /// single-precision numbers.
    pub fn to_clip(&self) -> Result<(Clip, Vec<bool>), ClipV1Error> {
        let duration = f64::from(self.duration_ms) * NANOS_PER_MILLI as f64;
        let mut narrow = Vec::with_capacity(self.tracks.len());
        let mut tracks = Vec::with_capacity(self.tracks.len());
        for track in &self.tracks {
            let not_finite = |k: &KeypointV1| ClipV1Error::NotFinite {
                track: track.id.clone(),
                keypoint: k.id.clone(),
            };
            let mut points: Vec<&KeypointV1> = track.points.iter().collect();
            if let Some(k) = points.iter().find(|k| !finite_point(k)) {
                return Err(not_finite(k));
            }
            points.sort_by(|a, b| a.stamp.total_cmp(&b.stamp));
            let time =
                |k: &KeypointV1| -> Nanos { (f64::from(k.stamp) * duration).round() as Nanos };
            let mut keypoints: Vec<Keypoint> = points
                .iter()
                .map(|k| Keypoint {
                    id: k.id.clone(),
                    time: time(k),
                    value: k.value.value(),
                    transitions: Transitions::default(),
                })
                .collect();
            for i in 0..points.len().saturating_sub(1) {
                let (a, b) = (points[i], points[i + 1]);
                let (Some(v0), Some(v1)) = (
                    keypoints[i].value.as_number(),
                    keypoints[i + 1].value.as_number(),
                ) else {
                    continue;
                };
                let dt = (keypoints[i + 1].time - keypoints[i].time) as f64;
                if dt == 0.0 {
                    continue;
                }
                let dv = v1 - v0;
                let (x1, y1) = a.leave.unwrap_or(DEFAULT_LEAVE);
                let (x2, y2) = b.arrive.unwrap_or(DEFAULT_ARRIVE);
                keypoints[i].transitions.leave = Some(Transition::Handle {
                    dt: f64::from(x1) * dt,
                    dv: f64::from(y1) * dv,
                });
                keypoints[i + 1].transitions.arrive = Some(Transition::Handle {
                    dt: (f64::from(x2) - 1.0) * dt,
                    dv: (f64::from(y2) - 1.0) * dv,
                });
            }
            narrow.push(
                !track.points.is_empty()
                    && track
                        .points
                        .iter()
                        .all(|k| matches!(k.value, ValueV1::F32(_))),
            );
            tracks.push(Track {
                id: track.id.clone(),
                name: track.name.clone(),
                target: track.animatable_id.clone(),
                keypoints,
            });
        }
        Ok((
            Clip {
                name: self.name.clone(),
                length: Some(duration as Nanos),
                tracks,
            },
            narrow,
        ))
    }
}

fn finite_point(k: &KeypointV1) -> bool {
    let finite = |h: Option<(f32, f32)>| h.is_none_or(|(x, y)| x.is_finite() && y.is_finite());
    k.stamp.is_finite() && finite(k.arrive) && finite(k.leave)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;

    fn point(
        id: &str,
        stamp: f32,
        value: ValueV1,
        leave: Option<(f32, f32)>,
        arrive: Option<(f32, f32)>,
    ) -> KeypointV1 {
        KeypointV1 {
            id: id.into(),
            stamp,
            value,
            arrive,
            leave,
        }
    }

    /// vizij-animation-core's 1.x evaluation of a number segment: the timing
    /// curve inverted, then a linear blend.
    fn v1_value(v0: f64, v1: f64, (x1, y1): (f64, f64), (x2, y2): (f64, f64), lt: f64) -> f64 {
        let cubic = |p1: f64, p2: f64, s: f64| {
            let m = 1.0 - s;
            3.0 * p1 * m * m * s + 3.0 * p2 * m * s * s + s * s * s
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..64 {
            let mid = 0.5 * (lo + hi);
            if cubic(x1, x2, mid) < lt {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        v0 + (v1 - v0) * cubic(y1, y2, 0.5 * (lo + hi))
    }

    #[test]
    fn a_normalized_segment_plays_the_same_curve_in_absolute_time() {
        let clip = ClipV1 {
            name: "c".into(),
            duration_ms: 2_000,
            tracks: vec![TrackV1 {
                id: "t".into(),
                name: "t".into(),
                animatable_id: "x".into(),
                points: vec![
                    point("a", 0.25, ValueV1::F32(1.0), Some((0.1, 0.9)), None),
                    point("b", 0.75, ValueV1::F32(-3.0), None, Some((0.8, 0.2))),
                    point("c", 1.0, ValueV1::F32(2.0), None, None),
                ],
            }],
        };
        let (converted, narrow) = clip.to_clip().unwrap();
        assert_eq!(narrow, [true]);
        assert_eq!(converted.end(), 2_000_000_000);
        let curve = Curve::new(&converted.tracks[0]);
        for i in 0..=100 {
            let lt = f64::from(i) / 100.0;
            let t = 500e6 + lt * 1000e6;
            let expected = v1_value(
                1.0,
                -3.0,
                (0.1f32 as f64, 0.9f32 as f64),
                (0.8f32 as f64, 0.2f32 as f64),
                lt,
            );
            let got = curve.sample(t).unwrap().value.as_number().unwrap();
            assert!((got - expected).abs() < 1e-9, "{lt}: {got} vs {expected}");
            let lt2 = lt;
            let t2 = 1500e6 + lt2 * 500e6;
            let expected = v1_value(-3.0, 2.0, (0.42f32 as f64, 0.0), (0.58f32 as f64, 1.0), lt2);
            let got = curve.sample(t2).unwrap().value.as_number().unwrap();
            assert!((got - expected).abs() < 1e-9, "{lt2}: {got} vs {expected}");
        }
        // Held outside its keypoints, as 1.x clamps its time.
        assert_eq!(curve.sample(0.0).unwrap().value, Value::Number(1.0));
    }

    #[test]
    fn a_stamp_that_is_not_finite_is_refused() {
        let clip = ClipV1 {
            name: "c".into(),
            duration_ms: 1_000,
            tracks: vec![TrackV1 {
                id: "t".into(),
                name: "t".into(),
                animatable_id: "x".into(),
                points: vec![point("a", f32::NAN, ValueV1::Bool(true), None, None)],
            }],
        };
        assert!(clip.to_clip().is_err());
    }
}
