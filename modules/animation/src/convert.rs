//! Conversions accept only the scalar vocabulary the animation library plays.
use crate::*;
use arora_animation::{clip as core, clip1, Value as Scalar};
use arora_types::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionError(pub &'static str);

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ConversionError {}

pub(crate) fn scalar(value: Value) -> Result<Scalar, ConversionError> {
    Ok(match value {
        Value::F32(v) => Scalar::Number(f64::from(v)),
        Value::F64(v) => Scalar::Number(v),
        Value::U8(v) => Scalar::Number(v.into()),
        Value::U16(v) => Scalar::Number(v.into()),
        Value::U32(v) => Scalar::Number(v.into()),
        Value::U64(v) => Scalar::Number(v as f64),
        Value::I8(v) => Scalar::Number(v.into()),
        Value::I16(v) => Scalar::Number(v.into()),
        Value::I32(v) => Scalar::Number(v.into()),
        Value::I64(v) => Scalar::Number(v as f64),
        Value::Boolean(v) => Scalar::Bool(v),
        Value::String(v) => Scalar::Text(v),
        _ => {
            return Err(ConversionError(
                "a keypoint must be a number, boolean or string",
            ))
        }
    })
}

pub(crate) fn value(value: Scalar, narrow: bool) -> Value {
    match value {
        Scalar::Number(v) if narrow => Value::F32(v as f32),
        Scalar::Number(v) => Value::F64(v),
        Scalar::Bool(v) => Value::Boolean(v),
        Scalar::Text(v) => Value::String(v),
    }
}

impl TryFrom<AnimationClip> for clip1::ClipV1 {
    type Error = ConversionError;
    fn try_from(clip: AnimationClip) -> Result<Self, Self::Error> {
        Ok(Self {
            name: clip.name,
            duration_ms: clip.duration,
            tracks: clip
                .tracks
                .into_iter()
                .map(|t| {
                    Ok(clip1::TrackV1 {
                        id: t.id,
                        name: t.name,
                        animatable_id: t.animatable_id,
                        points: t
                            .points
                            .into_iter()
                            .map(|k| {
                                let value = match k.value {
                                    Value::F32(v) => clip1::ValueV1::F32(v),
                                    v => match scalar(v)? {
                                        Scalar::Number(v) => clip1::ValueV1::F64(v),
                                        Scalar::Bool(v) => clip1::ValueV1::Bool(v),
                                        Scalar::Text(v) => clip1::ValueV1::Text(v),
                                    },
                                };
                                Ok(clip1::KeypointV1 {
                                    id: k.id,
                                    stamp: k.stamp,
                                    value,
                                    arrive: k.transitions_in.first().map(|h| (h.x, h.y)),
                                    leave: k.transitions_out.first().map(|h| (h.x, h.y)),
                                })
                            })
                            .collect::<Result<_, ConversionError>>()?,
                    })
                })
                .collect::<Result<_, ConversionError>>()?,
        })
    }
}

impl TryFrom<ClipTransition> for core::Transition {
    type Error = ConversionError;
    fn try_from(t: ClipTransition) -> Result<Self, Self::Error> {
        if t.kind == "handle" {
            Ok(Self::Handle {
                dt: t.dt_ns,
                dv: t.dv,
            })
        } else {
            Self::named(&t.kind).ok_or(ConversionError("unknown transition kind"))
        }
    }
}
impl From<core::Transition> for ClipTransition {
    fn from(t: core::Transition) -> Self {
        match t {
            core::Transition::Handle { dt, dv } => Self {
                kind: "handle".into(),
                dt_ns: dt,
                dv,
            },
            t => Self {
                kind: t.name().expect("a named transition").into(),
                dt_ns: 0.0,
                dv: 0.0,
            },
        }
    }
}

fn transition(side: Vec<ClipTransition>) -> Result<Option<core::Transition>, ConversionError> {
    if side.len() > 1 {
        return Err(ConversionError(
            "a keypoint side has zero or one transition",
        ));
    }
    side.into_iter().next().map(TryInto::try_into).transpose()
}
impl TryFrom<Clip> for core::Clip {
    type Error = ConversionError;
    fn try_from(clip: Clip) -> Result<Self, Self::Error> {
        Ok(Self {
            name: clip.name,
            length: clip.length_ns,
            tracks: clip
                .tracks
                .into_iter()
                .map(|t| {
                    Ok(core::Track {
                        id: t.id,
                        name: t.name,
                        target: t.target,
                        keypoints: t
                            .keypoints
                            .into_iter()
                            .map(|k| {
                                Ok(core::Keypoint {
                                    id: k.id,
                                    time: k.time_ns,
                                    value: scalar(k.value)?,
                                    transitions: core::Transitions {
                                        arrive: transition(k.arrive)?,
                                        leave: transition(k.leave)?,
                                        pairing: match k.pairing.as_str() {
                                            "" => None,
                                            "paired" => Some(core::Pairing::Paired),
                                            "free" => Some(core::Pairing::Free),
                                            _ => return Err(ConversionError("unknown pairing")),
                                        },
                                    },
                                })
                            })
                            .collect::<Result<_, ConversionError>>()?,
                    })
                })
                .collect::<Result<_, ConversionError>>()?,
        })
    }
}
impl From<core::Clip> for Clip {
    fn from(clip: core::Clip) -> Self {
        Self {
            name: clip.name,
            length_ns: clip.length,
            tracks: clip
                .tracks
                .into_iter()
                .map(|t| ClipTrack {
                    id: t.id,
                    name: t.name,
                    target: t.target,
                    keypoints: t
                        .keypoints
                        .into_iter()
                        .map(|k| ClipKeypoint {
                            id: k.id,
                            time_ns: k.time,
                            value: value(k.value, false),
                            arrive: k.transitions.arrive.into_iter().map(Into::into).collect(),
                            leave: k.transitions.leave.into_iter().map(Into::into).collect(),
                            pairing: match k.transitions.pairing {
                                None => "",
                                Some(core::Pairing::Paired) => "paired",
                                Some(core::Pairing::Free) => "free",
                            }
                            .into(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}
