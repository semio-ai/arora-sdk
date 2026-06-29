//! Prototype for generalizing semio-record's "record" notion (versioning +
//! freezing) behind a small trait set, so it applies to arbitrary arora types
//! without hard-wiring to module records and without copying semio-record.
//!
//! Core idea, distilled from semio-record:
//!   * An *unfrozen* value carries `UnfrozenReference { id, version_req }`s.
//!   * A *frozen* value carries `FrozenReference { id, version }`s — every
//!     dependency pinned to one concrete version.
//!   * "Freezing" = walking the value graph and resolving every
//!     unfrozen reference to a frozen one via a `Resolver` (semio-record calls
//!     it `Freezer`). The resolver is the ONLY part that needs a store/registry.
//!
//! This prototype generalizes that to ANY type via two orthogonal traits:
//!   * `Versioned`        — a type that has an identity + version + compat rules.
//!   * `Freeze<R>`        — "I can be frozen by resolver R into Self::Frozen".
//!   * `Resolver`         — "I can resolve an unfrozen ref to a frozen ref".
//!
//! Unlike semio-record, `Resolver`/`Freeze` here are **synchronous** and the
//! reference types are generic, so the same machinery freezes module headers,
//! structures, enumerations, behavior-tree types, etc. The async, store-backed
//! resolver stays Semio-private and just implements `Resolver`.

pub mod reference;
pub mod versioned;
pub mod freeze;
pub mod examples;

pub use freeze::{Freeze, Resolver};
pub use reference::{FrozenReference, UnfrozenReference, Version, VersionReq};
pub use versioned::{Compat, Versioned};
