//! ROS 2 message types for Arora.
//!
//! This crate lets an Arora device speak ROS 2: it carries the standard message
//! packages (`std_msgs`, `geometry_msgs`, `sensor_msgs`, `hri_msgs`, …) as arora
//! types, a registry that maps them by name and id, and the CDR codec that moves
//! a [`Value`](arora_types::value::Value) on and off the ROS 2 wire. It is shared
//! by [`arora-bridge-ros2`] (ROS as a device's remote data plane) and
//! [`arora-hal-ros2`] (ROS as a device's own hardware), so both agree on one set
//! of message types and one codec.
//!
//! # One type, four faces
//!
//! Every ROS message reduces to a single thing — an arora [`low::Type`] carrying
//! its ROS-qualified name (`geometry_msgs/msg/Point`) — from which everything
//! else follows:
//!
//! - **on the wire**: [`cdr::encode`]/[`cdr::decode`] turn a `Value` into ROS 2
//!   CDR bytes and back, type-directed by that `low::Type` (no per-message Rust);
//! - **its identity**: [`hash::rihs01`] (the `interop` feature) computes the
//!   message's REP-2016 type hash, so an arora publisher reaches native C++ ROS
//!   nodes;
//! - **in Rust**: for the bundled messages, a generated struct
//!   (`geometry_msgs::Point`) you can hold and pass around directly;
//! - **by name**: [`Ros2Registry`] resolves `"geometry_msgs/Point"` to the type,
//!   the operation a node editor needs to type a key.
//!
//! [`low::Type`]: arora_types::ty::low::Type
//!
//! # Types defined at runtime
//!
//! The registry is not limited to the bundled messages. A ROS developer can
//! [`define`](Ros2Registry::define) a type that was never a `.msg` here, and a
//! behavior can [`define_from_json`](Ros2Registry::define_from_json) a type it
//! authored on the fly — a JSON blob that is really just a [`low::Type`]. Either
//! becomes immediately encodable, hashable and publishable; using it on a topic
//! makes it visible to ROS clients when a ROS bridge is enabled.
//!
//! # Features
//!
//! The core — codec, registry, generated types, [`ros2_representable`] — depends
//! on no ROS middleware, so a modeling consumer (the editor, a wasm build) stays
//! lean. The `dds` / `zenoh` features add `interop`: the REP-2016 hash and the
//! live-ROS surface, pulling `ros2-client` with the matching backend.
//!
//! [`arora-bridge-ros2`]: https://github.com/semio-ai/arora-sdk
//! [`arora-hal-ros2`]: https://github.com/semio-ai/arora-sdk

pub mod cdr;
pub mod message;
pub mod registry;
pub mod representable;
pub mod schema;

#[cfg(feature = "interop")]
pub mod hash;

pub use cdr::{decode, encode};
pub use message::RosMessage;
pub use registry::{package_and_type, Ros2Registry};
pub use representable::{ros2_representable, NotRepresentable};

#[cfg(feature = "interop")]
pub use hash::rihs01;
#[cfg(feature = "interop")]
pub use message::type_hash;

// The bundled ROS 2 message types, generated from `msgs/**/*.msg` by `build.rs`:
// one module per package (so `arora_msgs_ros2::geometry_msgs::Point` is the
// generated struct) plus `registry()`, every bundled type at once.
include!(concat!(env!("OUT_DIR"), "/generated.rs"));
