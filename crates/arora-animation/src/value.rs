//! The values a track holds and plays: numbers, booleans and strings.

use serde::{Deserialize, Serialize};

/// A keypoint's value, and a track's value at a time.
///
/// In a clip's document form a value is the JSON value itself: a number, a
/// boolean or a string.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// A boolean, which plays as a step.
    Bool(bool),
    /// A number, in `f64`, interpolated along the track's curve.
    Number(f64),
    /// A string, which plays as a step.
    Text(String),
}

/// What a track's values are: one kind for every keypoint of a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Number,
    Bool,
    Text,
}

impl Value {
    /// The kind of this value.
    pub fn kind(&self) -> Kind {
        match self {
            Value::Number(_) => Kind::Number,
            Value::Bool(_) => Kind::Bool,
            Value::Text(_) => Kind::Text,
        }
    }

    /// The number this value holds, if it is one.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Number => "number",
            Kind::Bool => "boolean",
            Kind::Text => "string",
        })
    }
}

/// A track's value at a time and its rate of change there.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub value: Value,
    /// Value units per second of the time the track was evaluated in;
    /// `None` for a boolean or a string, which have no rate of change.
    pub velocity: Option<f64>,
}
