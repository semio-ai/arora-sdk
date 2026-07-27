//! The ROS 2 message types this HAL speaks, re-exported from `arora-msgs-ros2`.
//!
//! The message structs — and their compile-time ROS identity via [`RosMessage`]
//! (`PACKAGE`/`TYPE_NAME`/`ROS_TYPE_NAME`) — are generated once in
//! `arora-msgs-ros2` from the vendored `.msg` files and shared with
//! `arora-bridge-ros2`. This module only re-exports the subset the HAL names, so
//! existing `crate::msgs::JointState` paths keep resolving and both crates agree
//! on one set of types and one CDR codec.

pub use arora_msgs_ros2::RosMessage;

pub use arora_msgs_ros2::builtin_interfaces::{Duration, Time};
pub use arora_msgs_ros2::geometry_msgs::{
    Point, Pose, PoseStamped, Quaternion, Twist, TwistStamped, Vector3,
};
pub use arora_msgs_ros2::naoqi_bridge_msgs::JointAnglesWithSpeed;
pub use arora_msgs_ros2::sensor_msgs::JointState;
pub use arora_msgs_ros2::std_msgs::{
    Bool, Empty, Float64, Float64MultiArray, Header, Int32, MultiArrayDimension, MultiArrayLayout,
    String, UInt32,
};
pub use arora_msgs_ros2::trajectory_msgs::{JointTrajectory, JointTrajectoryPoint};
