//! The Semio-store-backed registries: the private half of `arora-registry`.
//!
//! `arora-registry` is the publishable, local-only registry; this crate keeps
//! the remote (network) registries that speak to the Semio store through the
//! private `semio-client`, so the git dependency never blocks publishing the
//! rest of the workspace. It implements the same `ReadableRegistry` /
//! `Resolver` traits, so consumers swap registries without code changes.

pub mod config;
pub mod remote;
pub mod remote_cached;
