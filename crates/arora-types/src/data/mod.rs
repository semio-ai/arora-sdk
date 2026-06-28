//! The Arora data interface.
//!
//! The shared, path-keyed blackboard that the HAL, the bridge, and execution
//! engines (behavior tree, modules) all agree on:
//!
//! - the vocabulary — [`Key`], [`State`], [`StateChange`] — lifted from
//!   `studio-bridge` so every consumer shares one definition;
//! - the [`DataStore`] trait (a shared, observable store) with a [`Slot`]
//!   direct-handle and a lean [`Subscription`] change feed.
//!
//! The canonical lean implementation lives in the `arora-simple-data-store`
//! crate (a simple hashmap store); richer backends can implement [`DataStore`]
//! too.
pub mod state;
pub mod store;

pub use state::{Change, Key, State, StateChange};
pub use store::{DataError, DataStore, Slot, Subscription};
