//! ROS 2 as an Arora [`Bridge`](arora_bridge::Bridge).
//!
//! An Arora device is one blackboard with four seams around it (store, HAL,
//! bridge, behavior). This crate is a **bridge** whose remote is a ROS 2 graph:
//! it exposes the device's keys over ROS 2 topics and accepts commands from
//! them. Keys speak the Arora data vocabulary — a [`Value`] at a hierarchical
//! **key** (e.g. `face/mouth`) — and each key maps to the topic
//! `/{namespace}/keys/{path}`.
//!
//! This is distinct from `arora-hal-ros2`: there ROS is the device's own
//! hardware (a HAL); here ROS is the remote control/data plane (a Bridge),
//! sibling to `arora-bridge-ws`.
//!
//! # Direction of flow
//!
//! - [`send_data`](arora_bridge::Bridge::send_data) publishes each changed key
//!   to its topic; the `std_msgs` message type is chosen from the value's type,
//!   with a JSON `std_msgs/String` fallback for non-scalar values.
//! - [`commands`](arora_bridge::Bridge::commands) turns each message received on
//!   a configured input topic into a
//!   [`BridgeOp::Update`](arora_bridge::BridgeOp::Update) for the Arora runtime
//!   to apply to its store.
//!
//! # Example
//!
//! ```rust,no_run
//! use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type};
//!
//! # async fn example() {
//! let config = Ros2BridgeConfig::new("robot", 0)
//!     .with_input("face/mouth/open", Type::F64)
//!     .with_input("enabled", Type::Boolean);
//! let bridge = Ros2Bridge::new(config).await;
//! // Hand `bridge` to the Arora runtime as its `Bridge`.
//! # let _ = bridge;
//! # }
//! ```

pub mod bridge;
pub mod conversions;
pub mod msg_types;

pub use bridge::{InputKey, Ros2Bridge, Ros2BridgeConfig};

pub use arora_types::value::{Type, Value};
