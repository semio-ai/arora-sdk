//! The Arora data interface.
//!
//! The shared, path-keyed blackboard vocabulary — [`Key`], [`State`],
//! [`StateChange`] — that the HAL, the bridge, and execution engines (behavior
//! tree, modules) all agree on. Lifted from `studio-bridge` so every consumer
//! shares one definition.
//!
//! A `DataStore` trait (an abstraction over a live, subscribable store) plus an
//! in-memory implementation are the next slice; see
//! `docs/plan-bring-studio-bridge-in.md` (the trait shape is a review checkpoint).
pub mod state;

pub use state::{Change, Key, State, StateChange};
