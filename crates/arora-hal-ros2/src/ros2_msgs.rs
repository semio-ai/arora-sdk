//! Message type definitions for common ROS2 messages.
//!
//! This module provides definitions for common ROS2 message types.
#![allow(dead_code)]
#![allow(unused_imports)]

use ros2_client::builtin_interfaces::Time;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// A type that can be used as a ROS2 message.
///
/// This trait is implemented for all ROS2 message types provided by ros2-client.
pub trait MessageType:
    Clone + Debug + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static
{
    /// Get the ROS2 message type name in the format "package/Type".
    const MESSAGE_TYPE_STR: &'static str;

    /// Get the ROS2 message type name as a `MessageTypeName`.
    fn message_type_name() -> ros2_client::MessageTypeName;
}

/// Implement the `MessageType` trait for a ROS2 message type.
///
/// This macro takes care of the common implementation details for
/// ROS2 message types, including the `message_type()` method.
macro_rules! impl_message_type {
    ($package:expr, $struct_name:ident) => {
        impl MessageType for $struct_name {
            const MESSAGE_TYPE_STR: &'static str = concat!($package, "/", stringify!($struct_name));
            fn message_type_name() -> ros2_client::MessageTypeName {
                ros2_client::MessageTypeName::new($package, stringify!($struct_name))
            }
        }
    };
}

// ====== std_msgs ======

/// String message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/String.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct String {
    pub data: std::string::String,
}
impl_message_type!("std_msgs", String);

/// Bool message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/Bool.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Bool {
    pub data: bool,
}
impl_message_type!("std_msgs", Bool);

/// Int32 message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/Int32.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Int32 {
    pub data: i32,
}
impl_message_type!("std_msgs", Int32);

/// UInt32 message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/UInt32.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct UInt32 {
    pub data: u32,
}
impl_message_type!("std_msgs", UInt32);

/// Float64 message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/Float64.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Float64 {
    pub data: f64,
}
impl_message_type!("std_msgs", Float64);

/// Empty message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/Empty.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Empty {}
impl_message_type!("std_msgs", Empty);

/// Header message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/Header.html
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub stamp: Time,
    pub frame_id: std::string::String,
}
impl_message_type!("std_msgs", Header);

impl Default for Header {
    fn default() -> Self {
        Self {
            stamp: Time::from_nanos(0),
            frame_id: std::string::String::default(),
        }
    }
}

/// MultiArrayDimension message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/MultiArrayDimension.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct MultiArrayDimension {
    pub label: std::string::String,
    pub size: u32,
    pub stride: u32,
}
impl_message_type!("std_msgs", MultiArrayDimension);

/// MultiArrayLayout message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/MultiArrayLayout.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct MultiArrayLayout {
    pub dim: Vec<MultiArrayDimension>,
    pub data_offset: u32,
}
impl_message_type!("std_msgs", MultiArrayLayout);

/// Float64MultiArray message from std_msgs
///
/// http://docs.ros.org/en/noetic/api/std_msgs/html/msg/Float64MultiArray.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Float64MultiArray {
    pub layout: MultiArrayLayout,
    pub data: Vec<f64>,
}
impl_message_type!("std_msgs", Float64MultiArray);

// ====== sensor_msgs ======

/// JointState message from sensor_msgs
///
/// http://docs.ros.org/en/noetic/api/sensor_msgs/html/msg/JointState.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct JointState {
    pub header: Header,
    pub name: Vec<std::string::String>,
    pub position: Vec<f64>,
    pub velocity: Vec<f64>,
    pub effort: Vec<f64>,
}
impl_message_type!("sensor_msgs", JointState);

// ====== geometry_msgs ======

/// Point message from geometry_msgs
///
/// http://docs.ros.org/en/noetic/api/geometry_msgs/html/msg/Point.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl_message_type!("geometry_msgs", Point);

/// Quaternion message from geometry_msgs
///
/// http://docs.ros.org/en/noetic/api/geometry_msgs/html/msg/Quaternion.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}
impl_message_type!("geometry_msgs", Quaternion);

/// Pose message from geometry_msgs
///
/// http://docs.ros.org/en/noetic/api/geometry_msgs/html/msg/Pose.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Pose {
    pub position: Point,
    pub orientation: Quaternion,
}
impl_message_type!("geometry_msgs", Pose);

/// PoseStamped message from geometry_msgs
///
/// http://docs.ros.org/en/noetic/api/geometry_msgs/html/msg/PoseStamped.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PoseStamped {
    pub header: Header,
    pub pose: Pose,
}
impl_message_type!("geometry_msgs", PoseStamped);

/// Vector3 message from geometry_msgs
///
/// http://docs.ros.org/en/noetic/api/geometry_msgs/html/msg/Vector3.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl_message_type!("geometry_msgs", Vector3);

/// Twist message from geometry_msgs
///
/// http://docs.ros.org/en/noetic/api/geometry_msgs/html/msg/Twist.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Twist {
    pub linear: Vector3,
    pub angular: Vector3,
}
impl_message_type!("geometry_msgs", Twist);

/// TwistStamped message from geometry_msgs
///
/// http://docs.ros.org/en/noetic/api/geometry_msgs/html/msg/TwistStamped.html
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TwistStamped {
    pub header: Header,
    pub twist: Twist,
}
impl_message_type!("geometry_msgs", TwistStamped);

// Re-export std_msgs for backwards compatibility
pub mod std_msgs {
    pub use super::{
        Bool, Empty, Float64, Float64MultiArray, Header, Int32, MultiArrayDimension,
        MultiArrayLayout, String, UInt32,
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Duration {
    pub sec: i32,
    pub nanosec: u32,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct JointTrajectoryPoint {
    pub positions: Vec<f64>,
    pub velocities: Vec<f64>,
    pub accelerations: Vec<f64>,
    pub effort: Vec<f64>,
    pub time_from_start: Duration,
}
impl_message_type!("trajectory_msgs", JointTrajectoryPoint);

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct JointTrajectory {
    pub header: Header,
    pub joint_names: Vec<std::string::String>,
    pub points: Vec<JointTrajectoryPoint>,
}
impl_message_type!("trajectory_msgs", JointTrajectory);

// Re-export sensor_msgs
pub mod sensor_msgs {
    pub use super::JointState;
}

// Re-export geometry_msgs
pub mod geometry_msgs {
    pub use super::{Point, Pose, PoseStamped, Quaternion, Twist, TwistStamped, Vector3};
}

// Re-export trajectory_msgs
pub mod trajectory_msgs {
    pub use super::{JointTrajectory, JointTrajectoryPoint};
}

// ====== naoqi_bridge_msgs ======

/// JointAnglesWithSpeed message from naoqi_bridge_msgs
///
/// Custom message type for NAO/Pepper robot joint control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JointAnglesWithSpeed {
    pub header: Header,
    /// A list of joint names, corresponding to their names in the Nao docs.
    /// This must either have the same length as joint_angles or
    /// length 1 if it's a keyword such as 'Body' (for all angles)
    pub joint_names: Vec<std::string::String>,
    pub joint_angles: Vec<f32>,
    /// fraction of max joint velocity [0:1]
    pub speed: f32,
    /// Absolute angle (=0, default) or relative change
    pub relative: u8,
}
impl_message_type!("naoqi_bridge_msgs", JointAnglesWithSpeed);

// Custom Default implementation for JointAnglesWithSpeed
impl Default for JointAnglesWithSpeed {
    fn default() -> Self {
        Self {
            header: Header::default(),
            joint_names: Vec::default(),
            joint_angles: Vec::default(),
            speed: 0.1,  // Default to 10% of max velocity
            relative: 0, // Default to absolute positioning
        }
    }
}

// Re-export naoqi_bridge_msgs
pub mod naoqi_bridge_msgs {
    pub use super::JointAnglesWithSpeed;
}

// Re-export builtin_interfaces
pub mod builtin_interfaces {
    pub use ros2_client::builtin_interfaces::Time;
}
