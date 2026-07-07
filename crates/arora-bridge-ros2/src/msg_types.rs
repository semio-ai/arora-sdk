//! Native ROS 2 message wrappers compatible with `std_msgs`.
//!
//! These structs match the CDR serialization layout of the standard `std_msgs`
//! scalar types, so a key published or subscribed here interoperates directly
//! with the `ros2` CLI tools and other ROS 2 nodes.
//!
//! The [`MessageType`] trait and the `impl_message_type!` macro associate each
//! Rust struct with its ROS 2 type name (e.g. `"std_msgs/Float64"`) at compile
//! time, for DDS topic creation.

use ros2_client::Message;
use serde::{Deserialize, Serialize};

/// A ROS 2 message type with a compile-time type name.
pub trait MessageType:
    Clone + std::fmt::Debug + Send + Sync + Serialize + serde::de::DeserializeOwned + 'static
{
    /// The ROS 2 message type name in `"package/Type"` format.
    const MESSAGE_TYPE_STR: &'static str;

    /// Build a [`ros2_client::MessageTypeName`] for DDS topic creation.
    fn message_type_name() -> ros2_client::MessageTypeName;
}

/// Implement [`MessageType`] (and the marker [`Message`]) for a struct in this
/// module.
macro_rules! impl_message_type {
    ($package:expr, $struct_name:ident) => {
        impl MessageType for $struct_name {
            const MESSAGE_TYPE_STR: &'static str = concat!($package, "/", stringify!($struct_name));
            fn message_type_name() -> ros2_client::MessageTypeName {
                ros2_client::MessageTypeName::new($package, stringify!($struct_name))
            }
        }
        impl Message for $struct_name {}
    };
}

/// `std_msgs/Float64`
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Float64 {
    pub data: f64,
}
impl_message_type!("std_msgs", Float64);

/// `std_msgs/Float32`
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Float32 {
    pub data: f32,
}
impl_message_type!("std_msgs", Float32);

/// `std_msgs/Int64`
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Int64 {
    pub data: i64,
}
impl_message_type!("std_msgs", Int64);

/// `std_msgs/Int32`
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Int32 {
    pub data: i32,
}
impl_message_type!("std_msgs", Int32);

/// `std_msgs/UInt64`
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct UInt64 {
    pub data: u64,
}
impl_message_type!("std_msgs", UInt64);

/// `std_msgs/UInt32`
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct UInt32 {
    pub data: u32,
}
impl_message_type!("std_msgs", UInt32);

/// `std_msgs/Bool`
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Bool {
    pub data: bool,
}
impl_message_type!("std_msgs", Bool);

/// `std_msgs/String`
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct String {
    pub data: std::string::String,
}
impl_message_type!("std_msgs", String);
