//! (De)serialization to and from arora's [`Value`], in three layers:
//!
//! - [`bridge`] — the serde bridge: any `Serialize`/`Deserialize` Rust type
//!   converts to and from a [`Value`], the `serde_json::to_value`-style path
//!   with `Value` as the data model. [`to_value`]/[`from_value`] are the entry
//!   points. Ids come from hashing names.
//! - [`seeded`] — the bridge seeded with a declared [`low::Type`]: the same
//!   Rust ⇄ `Value` conversion, but ids come from the *type* rather than from
//!   names. [`to_value_seeded`]/[`from_value_seeded`] are the entry points.
//! - [`walk`] — the type-directed walk: a runtime recursion over a
//!   [`low::Type`] that drives a [`ValueWriter`]/[`ValueReader`] wire format
//!   (arora-buffers, ROS 2 CDR) over a [`Value`]. [`write_value`]/[`read_value`]
//!   are the entry points.
//!
//! [`Value`]: crate::value::Value
//! [`low::Type`]: crate::ty::low::Type
//! [`ValueWriter`]: walk::ValueWriter
//! [`ValueReader`]: walk::ValueReader

use std::fmt::{self, Display};

use serde::{de, ser};

pub mod bridge;
pub mod seeded;
pub mod walk;

pub use bridge::*;
pub use seeded::*;
pub use walk::*;

/// A registry that resolves the nested types a [`walk`] traverses. Re-exported
/// from [`crate::ty`], its canonical home.
pub use crate::ty::TypeRegistry;

/// The single error type of this module: a conversion between a Rust type and a
/// [`Value`] failed (serde bridge), or a [`Value`] did not match the
/// [`low::Type`] it was walked against / a wire datum did not match the
/// requested type / the buffer was malformed (type-directed walk).
///
/// [`Value`]: crate::value::Value
/// [`low::Type`]: crate::ty::low::Type
#[derive(Debug)]
pub struct Error(String);

impl Error {
  pub fn new(message: impl Into<String>) -> Self {
    Error(message.into())
  }
}

impl Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
  fn custom<T: Display>(msg: T) -> Self {
    Error(msg.to_string())
  }
}

impl de::Error for Error {
  fn custom<T: Display>(msg: T) -> Self {
    Error(msg.to_string())
  }
}

/// The result of a [`value_serde`](self) conversion.
pub type Result<T> = std::result::Result<T, Error>;
