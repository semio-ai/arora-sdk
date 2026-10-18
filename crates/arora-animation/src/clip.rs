//! Clip format 2.0: tracks of keypoints at signed nanoseconds of clip-local
//! time, each side of a keypoint carrying the transition its author chose.
//!
//! A clip states what an author made, not a curve resolved from it: a side
//! of a keypoint is an explicit handle in time and value units, an easing, or
//! a tangent inferred from the neighbouring keypoints. [`crate::curve`]
//! resolves them into the segments that play, by the rules Semio Studio's
//! editor draws them with ([`crate::resolve`]), so every reader of a clip
//! plays one curve.
//!
//! **The document form** is JSON, the core of a stored Studio animation
//! (`formatVersion` 3), whose other fields — the animation's id, its groups,
//! a track's colour — are the editor's and are ignored here:
//!
//! ```json
//! {
//!   "name": "wave",
//!   "lengthNs": 2000000000,
//!   "tracks": [{
//!     "id": "t1", "name": "elbow", "animatableId": "elbow",
//!     "points": [
//!       { "id": "k0", "timeNs": 0, "value": 0,
//!         "transitions": { "out": { "dtNs": 250000000, "dv": 0.5 }, "pairing": "paired" } },
//!       { "id": "k1", "timeNs": 1000000000, "value": 1, "transitions": { "in": "sine" } }
//!     ]
//!   }]
//! }
//! ```

use std::fmt;

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::time::Nanos;
use crate::value::{Kind, Value};

/// An animation clip: tracks, each animating one target.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    #[serde(default)]
    pub name: String,
    /// How long the clip plays when nothing else bounds it, in nanoseconds
    /// of clip-local time from 0: a player whose window is not set plays it
    /// to there. Absent, the clip plays to its last keypoint.
    #[serde(default, rename = "lengthNs", skip_serializing_if = "Option::is_none")]
    pub length: Option<Nanos>,
    pub tracks: Vec<Track>,
}

/// One animated target and its keypoints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// What the track animates: the key a player's output for it carries.
    #[serde(rename = "animatableId")]
    pub target: String,
    /// The keypoints, in any order: they play in time order, and keypoints
    /// sharing a time in the order given.
    #[serde(rename = "points")]
    pub keypoints: Vec<Keypoint>,
}

/// A value at a time, and how the curve arrives at and leaves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keypoint {
    pub id: String,
    /// Clip-local time, in nanoseconds; negative before the clip's 0.
    #[serde(rename = "timeNs")]
    pub time: Nanos,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Transitions::is_empty")]
    pub transitions: Transitions,
}

/// The transitions on either side of a keypoint, and whether the editor
/// keeps its two handles paired.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Transitions {
    /// How the curve arrives at the keypoint; absent, [`Transition::DEFAULT`].
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    pub arrive: Option<Transition>,
    /// How the curve leaves the keypoint; absent, [`Transition::DEFAULT`].
    #[serde(default, rename = "out", skip_serializing_if = "Option::is_none")]
    pub leave: Option<Transition>,
    /// An editing constraint the clip carries for the editor; it does not
    /// change how the clip plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing: Option<Pairing>,
}

impl Transitions {
    fn is_empty(&self) -> bool {
        self.arrive.is_none() && self.leave.is_none() && self.pairing.is_none()
    }
}

/// Whether an editor moves a keypoint's two handles together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pairing {
    Paired,
    Free,
}

/// How a curve arrives at or leaves a keypoint on a number track. A
/// boolean or string track steps, whatever its transitions say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transition {
    /// A Bézier control point, relative to its keypoint, in the clip's own
    /// units: `dt` nanoseconds of clip-local time and `dv` value units.
    Handle { dt: f64, dv: f64 },
    /// A standard easing, its control point placed on the segment by the
    /// easing's parameters.
    Easing(Easing),
    /// A tangent inferred from the neighbouring keypoints: Studio's
    /// `explicit-handles` directive, which seeds an explicit handle from the
    /// automatic tangent until the author moves it.
    Auto,
    /// The inferred tangent, flattened at a local extremum and held within
    /// its segment's value range: Studio's `inferred-auto-clamped`.
    AutoClamped,
}

impl Transition {
    /// The transition of a side that states none.
    pub const DEFAULT: Transition = Transition::Easing(Easing::Cubic);
}

/// The standard easings, by the names their document form spells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Easing {
    Linear,
    Sine,
    Quad,
    Cubic,
    Quart,
    Quint,
    Expo,
    Circ,
    Back,
}

impl Easing {
    pub const ALL: [Easing; 9] = [
        Easing::Linear,
        Easing::Sine,
        Easing::Quad,
        Easing::Cubic,
        Easing::Quart,
        Easing::Quint,
        Easing::Expo,
        Easing::Circ,
        Easing::Back,
    ];

    /// The name the document form spells this easing with.
    pub fn name(self) -> &'static str {
        match self {
            Easing::Linear => "linear",
            Easing::Sine => "sine",
            Easing::Quad => "quad",
            Easing::Cubic => "cubic",
            Easing::Quart => "quart",
            Easing::Quint => "quint",
            Easing::Expo => "expo",
            Easing::Circ => "circ",
            Easing::Back => "back",
        }
    }

    /// The easing a name spells.
    pub fn named(name: &str) -> Option<Easing> {
        Easing::ALL.into_iter().find(|e| e.name() == name)
    }
}

/// The document form's spelling of [`Transition::Auto`].
pub const AUTO_NAME: &str = "explicit-handles";
/// The document form's spelling of [`Transition::AutoClamped`].
pub const AUTO_CLAMPED_NAME: &str = "inferred-auto-clamped";

impl Transition {
    /// The transition a name spells: an easing, or one of the two inferred
    /// tangents.
    pub fn named(name: &str) -> Option<Transition> {
        match name {
            AUTO_NAME => Some(Transition::Auto),
            AUTO_CLAMPED_NAME => Some(Transition::AutoClamped),
            name => Easing::named(name).map(Transition::Easing),
        }
    }

    /// The name of a named transition; `None` for a handle.
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Transition::Handle { .. } => None,
            Transition::Easing(easing) => Some(easing.name()),
            Transition::Auto => Some(AUTO_NAME),
            Transition::AutoClamped => Some(AUTO_CLAMPED_NAME),
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum TransitionForm {
    Named(String),
    Handle {
        #[serde(rename = "dtNs")]
        dt: f64,
        dv: f64,
    },
}

impl Serialize for Transition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match *self {
            Transition::Handle { dt, dv } => TransitionForm::Handle { dt, dv },
            named => TransitionForm::Named(named.name().unwrap_or_default().to_string()),
        }
        .serialize(serializer)
    }
}

// Read straight from the document: an untagged derive buffers every
// transition and tries each form in turn, which makes it the larger part of
// compiling a track.
impl<'de> Deserialize<'de> for Transition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TransitionVisitor;

        impl<'de> Visitor<'de> for TransitionVisitor {
            type Value = Transition;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a transition's name or a handle {dtNs, dv}")
            }

            fn visit_str<E: de::Error>(self, name: &str) -> Result<Transition, E> {
                Transition::named(name).ok_or_else(|| {
                    E::custom(format!(
                        "unknown transition {name:?}: an easing ({}), {AUTO_NAME:?} or {AUTO_CLAMPED_NAME:?}",
                        Easing::ALL.map(|e| e.name()).join(", ")
                    ))
                })
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Transition, M::Error> {
                let (mut dt, mut dv) = (None, None);
                while let Some(key) = map.next_key::<HandleField>()? {
                    match key {
                        HandleField::Dt => dt = Some(map.next_value::<f64>()?),
                        HandleField::Dv => dv = Some(map.next_value::<f64>()?),
                        HandleField::Other => {
                            map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Transition::Handle {
                    dt: dt.ok_or_else(|| de::Error::missing_field("dtNs"))?,
                    dv: dv.ok_or_else(|| de::Error::missing_field("dv"))?,
                })
            }
        }

        deserializer.deserialize_any(TransitionVisitor)
    }
}

/// A key of a handle's document form; other keys are ignored.
#[derive(Deserialize)]
enum HandleField {
    #[serde(rename = "dtNs")]
    Dt,
    #[serde(rename = "dv")]
    Dv,
    #[serde(other)]
    Other,
}

/// Why a clip cannot play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipError {
    /// A track's keypoints are not all of one kind.
    MixedKinds {
        track: String,
        first: Kind,
        other: Kind,
    },
    /// A number that is not finite: a keypoint's value or a handle's.
    NotFinite { track: String, keypoint: String },
    /// A document that does not parse as a clip.
    Document(String),
}

impl std::fmt::Display for ClipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipError::MixedKinds {
                track,
                first,
                other,
            } => write!(
                f,
                "track {track:?} mixes {first} and {other} keypoints: a track's keypoints are of one kind"
            ),
            ClipError::NotFinite { track, keypoint } => write!(
                f,
                "keypoint {keypoint:?} of track {track:?} carries a number that is not finite"
            ),
            ClipError::Document(e) => write!(f, "not a clip: {e}"),
        }
    }
}

impl std::error::Error for ClipError {}

impl Clip {
    /// A clip from its document form.
    pub fn from_json(json: &str) -> Result<Clip, ClipError> {
        let clip: Clip =
            serde_json::from_str(json).map_err(|e| ClipError::Document(e.to_string()))?;
        clip.validate()?;
        Ok(clip)
    }

    /// The clip's document form.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a clip serializes")
    }

    /// Checks what a clip must hold to play: each track's keypoints of one
    /// kind, and every number finite.
    pub fn validate(&self) -> Result<(), ClipError> {
        self.tracks.iter().try_for_each(Track::validate)
    }

    /// Where the clip ends when nothing else bounds it: its `length`, else
    /// its latest keypoint, and never before 0.
    pub fn end(&self) -> Nanos {
        self.length
            .unwrap_or_else(|| {
                self.tracks
                    .iter()
                    .flat_map(|t| t.keypoints.iter().map(|k| k.time))
                    .max()
                    .unwrap_or(0)
            })
            .max(0)
    }
}

impl Track {
    /// The kind of the track's values; `None` for a track with no keypoint.
    pub fn kind(&self) -> Option<Kind> {
        self.keypoints.first().map(|k| k.value.kind())
    }

    /// See [`Clip::validate`].
    pub fn validate(&self) -> Result<(), ClipError> {
        let Some(first) = self.kind() else {
            return Ok(());
        };
        for keypoint in &self.keypoints {
            let kind = keypoint.value.kind();
            if kind != first {
                return Err(ClipError::MixedKinds {
                    track: self.id.clone(),
                    first,
                    other: kind,
                });
            }
            let handle_finite = |t: &Option<Transition>| match t {
                Some(Transition::Handle { dt, dv }) => dt.is_finite() && dv.is_finite(),
                _ => true,
            };
            let finite = keypoint.value.as_number().is_none_or(f64::is_finite)
                && handle_finite(&keypoint.transitions.arrive)
                && handle_finite(&keypoint.transitions.leave);
            if !finite {
                return Err(ClipError::NotFinite {
                    track: self.id.clone(),
                    keypoint: keypoint.id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_form_reads_and_writes_back() {
        let json = r#"{"name":"wave","lengthNs":2000000000,"tracks":[{"id":"t1","name":"elbow","animatableId":"elbow","points":[{"id":"k0","timeNs":-5,"value":0.0,"transitions":{"out":{"dtNs":250000000.0,"dv":0.5},"pairing":"paired"}},{"id":"k1","timeNs":1000000000,"value":1.0,"transitions":{"in":"sine","out":"explicit-handles"}},{"id":"k2","timeNs":1500000000,"value":2.0,"transitions":{"in":"inferred-auto-clamped"}}]}]}"#;
        let clip = Clip::from_json(json).unwrap();
        assert_eq!(clip.length, Some(2_000_000_000));
        let points = &clip.tracks[0].keypoints;
        assert_eq!(points[0].time, -5);
        assert_eq!(
            points[0].transitions.leave,
            Some(Transition::Handle { dt: 2.5e8, dv: 0.5 })
        );
        assert_eq!(points[0].transitions.pairing, Some(Pairing::Paired));
        assert_eq!(
            points[1].transitions.arrive,
            Some(Transition::Easing(Easing::Sine))
        );
        assert_eq!(points[1].transitions.leave, Some(Transition::Auto));
        assert_eq!(points[2].transitions.arrive, Some(Transition::AutoClamped));
        assert_eq!(clip.to_json(), json);
    }

    #[test]
    fn the_editors_fields_are_ignored() {
        let json = r#"{"formatVersion":3,"id":"a","groups":[],"defaultViewportExtent":5000,"tracks":[{"id":"t","animatableId":"x","settings":{"color":"red"},"groupName":"g","points":[{"id":"k","timeNs":0,"value":"idle"}]}]}"#;
        let clip = Clip::from_json(json).unwrap();
        assert_eq!(
            clip.tracks[0].keypoints[0].value,
            Value::Text("idle".into())
        );
        assert_eq!(clip.end(), 0);
    }

    #[test]
    fn a_clip_that_cannot_play_is_refused() {
        let mixed = r#"{"tracks":[{"id":"t","animatableId":"x","points":[{"id":"a","timeNs":0,"value":1},{"id":"b","timeNs":1,"value":true}]}]}"#;
        assert_eq!(
            Clip::from_json(mixed),
            Err(ClipError::MixedKinds {
                track: "t".into(),
                first: Kind::Number,
                other: Kind::Bool
            })
        );
        let unknown = r#"{"tracks":[{"id":"t","animatableId":"x","points":[{"id":"a","timeNs":0,"value":1,"transitions":{"in":"bouncy"}}]}]}"#;
        assert!(
            matches!(Clip::from_json(unknown), Err(ClipError::Document(e)) if e.contains("bouncy"))
        );
        let half_handle = r#"{"tracks":[{"id":"t","animatableId":"x","points":[{"id":"a","timeNs":0,"value":1,"transitions":{"out":{"dtNs":5}}}]}]}"#;
        assert!(
            matches!(Clip::from_json(half_handle), Err(ClipError::Document(e)) if e.contains("dv"))
        );
        let no_value = r#"{"tracks":[{"id":"t","animatableId":"x","points":[{"id":"a","timeNs":0,"value":null}]}]}"#;
        assert!(matches!(
            Clip::from_json(no_value),
            Err(ClipError::Document(_))
        ));
        let clip = Clip {
            tracks: vec![Track {
                id: "t".into(),
                target: "x".into(),
                keypoints: vec![Keypoint {
                    id: "a".into(),
                    time: 0,
                    value: Value::Number(f64::INFINITY),
                    transitions: Transitions::default(),
                }],
                ..Track::default()
            }],
            ..Clip::default()
        };
        assert!(matches!(clip.validate(), Err(ClipError::NotFinite { .. })));
    }

    #[test]
    fn a_clip_ends_at_its_length_or_its_last_keypoint() {
        let mut clip = Clip::from_json(
            r#"{"tracks":[{"id":"t","animatableId":"x","points":[{"id":"a","timeNs":-9,"value":1},{"id":"b","timeNs":700,"value":2}]}]}"#,
        )
        .unwrap();
        assert_eq!(clip.end(), 700);
        clip.length = Some(5_000);
        assert_eq!(clip.end(), 5_000);
        clip.tracks[0].keypoints.retain(|k| k.time < 0);
        clip.length = None;
        assert_eq!(clip.end(), 0);
    }
}
