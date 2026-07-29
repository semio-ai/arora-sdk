//! Conversions between the Arora data vocabulary and native ROS 2 messages.
//!
//! A device's keys map to ROS 2 topics under a namespace: the key `face/mouth`
//! becomes the topic `/{namespace}/keys/face/mouth`. Inbound, a typed
//! subscription turns each received message into a [`StateChange`] for one key;
//! outbound, a [`KeyPublisher`] turns a [`Value`] into the matching `std_msgs`
//! message. The topic type is chosen from the value's type — scalars map to
//! their `std_msgs` counterpart, everything else falls back to a JSON-encoded
//! `std_msgs/String`.

use std::pin::Pin;
use std::sync::Arc;

use arora_msgs_ros2::{cdr, package_and_type, Ros2Registry};
use arora_types::data::{Key, StateChange};
use arora_types::ty::low;
use arora_types::value::{Type, Value};
use futures::stream::unfold;
use futures::Stream;
use log::warn;
#[cfg(feature = "zenoh")]
use ros2_client::QosProfile;
use ros2_client::{MessageTypeName, Name, Node, Publisher, RawPublisher};
#[cfg(feature = "dds")]
use ros2_client::{DEFAULT_PUBLISHER_QOS, DEFAULT_SUBSCRIPTION_QOS};
use tokio::time::{sleep, Duration};

use crate::msg_types::{
    Bool, Float32, Float64, Int32, Int64, MessageType, String as RosString, UInt32, UInt64,
};

/// A boxed stream of single-key [`StateChange`]s produced by one subscription.
pub type StateChangeStream = Pin<Box<dyn Stream<Item = StateChange> + Send>>;

/// The ROS 2 topic name a key is exposed on: `/{namespace}/keys/{path}`.
pub fn topic_name(namespace: &str, path: &str) -> String {
    format!("/{namespace}/keys/{path}")
}

/// Which `std_msgs` message type a [`Value`] is published as. Scalars map to
/// their native `std_msgs` type; anything else is JSON-encoded into a
/// `std_msgs/String`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosMsgKind {
    F64,
    F32,
    I64,
    I32,
    U64,
    U32,
    Bool,
    /// A `std_msgs/String` carrying the string verbatim.
    Str,
    /// A `std_msgs/String` carrying a JSON encoding of the value.
    Json,
}

/// Pick the ROS 2 message type for a value.
pub fn ros_msg_kind(value: &Value) -> RosMsgKind {
    match value {
        Value::F64(_) => RosMsgKind::F64,
        Value::F32(_) => RosMsgKind::F32,
        Value::I64(_) => RosMsgKind::I64,
        Value::I32(_) => RosMsgKind::I32,
        Value::U64(_) => RosMsgKind::U64,
        Value::U32(_) => RosMsgKind::U32,
        Value::Boolean(_) => RosMsgKind::Bool,
        Value::String(_) => RosMsgKind::Str,
        _ => RosMsgKind::Json,
    }
}

// =========================================================================
// Inbound: ROS 2 topic -> StateChange
// =========================================================================

/// Maximum number of consecutive `async_take` errors before a subscription's
/// stream terminates. Each error backs off exponentially (starting at 100 ms)
/// before retrying, so the total wait before termination stays bounded.
const MAX_CONSECUTIVE_ERRORS: u32 = 5;

/// Create a typed subscription for a key and return a stream that yields a
/// single-key [`StateChange`] for each received message. The value type
/// selects the `std_msgs` topic type; an unrecognised type falls back to a
/// JSON-encoded `std_msgs/String`.
pub fn setup_key_subscriber(
    node: &mut Node,
    namespace: &str,
    path: &str,
    value_type: &Type,
) -> Result<StateChangeStream, String> {
    let topic = topic_name(namespace, path);
    let path = path.to_string();

    match value_type {
        Type::F64 => setup_typed::<Float64>(node, &topic, path, |m| Value::F64(m.data)),
        Type::F32 => setup_typed::<Float32>(node, &topic, path, |m| Value::F32(m.data)),
        Type::I64 => setup_typed::<Int64>(node, &topic, path, |m| Value::I64(m.data)),
        Type::I32 => setup_typed::<Int32>(node, &topic, path, |m| Value::I32(m.data)),
        Type::U64 => setup_typed::<UInt64>(node, &topic, path, |m| Value::U64(m.data)),
        Type::U32 => setup_typed::<UInt32>(node, &topic, path, |m| Value::U32(m.data)),
        Type::Boolean => setup_typed::<Bool>(node, &topic, path, |m| Value::Boolean(m.data)),
        Type::String => setup_typed::<RosString>(node, &topic, path, |m| Value::String(m.data)),
        other => {
            warn!(
                "key '{path}' has unsupported type {other:?}, falling back to a JSON \
                 std_msgs/String topic"
            );
            setup_typed::<RosString>(node, &topic, path, |m| {
                serde_json::from_str::<Value>(&m.data).unwrap_or_else(|e| {
                    warn!("failed to parse JSON value from topic: {e}");
                    Value::String(m.data)
                })
            })
        }
    }
}

/// Create a typed subscription and return a stream converting each message to a
/// single-key [`StateChange`].
fn setup_typed<M: MessageType>(
    node: &mut Node,
    topic_name: &str,
    path: String,
    convert: impl Fn(M) -> Value + Send + Sync + 'static,
) -> Result<StateChangeStream, String> {
    let ros_name =
        Name::parse(topic_name).map_err(|e| format!("invalid topic name '{topic_name}': {e}"))?;

    #[cfg(feature = "dds")]
    let topic = node
        .create_topic(&ros_name, M::message_type_name(), &DEFAULT_SUBSCRIPTION_QOS)
        .map_err(|e| format!("failed to create topic {topic_name}: {e:?}"))?;
    // The Zenoh backend's `create_topic` is infallible (returns `Topic`).
    #[cfg(feature = "zenoh")]
    let topic = node.create_topic(
        &ros_name,
        M::message_type_name(),
        &QosProfile::subscription_default(),
    );

    let subscription = node
        .create_subscription::<M>(&topic, None)
        .map_err(|e| format!("failed to subscribe to {topic_name}: {e:?}"))?;

    let convert = Arc::new(convert);
    let stream = unfold((subscription, 0u32), move |(sub, errors)| {
        let path = path.clone();
        let convert = convert.clone();
        async move {
            let mut errors = errors;
            loop {
                match sub.async_take().await {
                    Ok((msg, _info)) => {
                        let mut change = StateChange::new();
                        change
                            .set
                            .insert(Key::from(path.clone()), Some(convert(msg)));
                        return Some((change, (sub, 0)));
                    }
                    Err(e) => {
                        errors += 1;
                        if errors >= MAX_CONSECUTIVE_ERRORS {
                            warn!(
                                "subscription for key '{path}' failed {MAX_CONSECUTIVE_ERRORS} \
                                 consecutive times, terminating stream"
                            );
                            return None;
                        }
                        // Cap the shift so `1u64 << shift` cannot overflow.
                        let shift = (errors - 1).min(62) as u64;
                        let delay = Duration::from_millis(100u64.saturating_mul(1u64 << shift));
                        warn!("subscription for key '{path}' errored: {e:?}; retrying");
                        sleep(delay).await;
                    }
                }
            }
        }
    });

    Ok(Box::pin(stream))
}

// =========================================================================
// Outbound: Value -> ROS 2 topic
// =========================================================================

/// A publisher for one key, typed to the `std_msgs` message chosen from the
/// first value published to it. Reused for subsequent values on the same key.
pub enum KeyPublisher {
    F64(Publisher<Float64>),
    F32(Publisher<Float32>),
    I64(Publisher<Int64>),
    I32(Publisher<Int32>),
    U64(Publisher<UInt64>),
    U32(Publisher<UInt32>),
    Bool(Publisher<Bool>),
    /// A `std_msgs/String` for `Value::String` (verbatim).
    Str(Publisher<RosString>),
    /// A `std_msgs/String` carrying a JSON encoding of any other value.
    Json(Publisher<RosString>),
}

impl KeyPublisher {
    /// Create a publisher on the key's topic, choosing the message type from a
    /// sample value.
    pub fn create(node: &mut Node, topic_name: &str, sample: &Value) -> Result<Self, String> {
        Ok(match ros_msg_kind(sample) {
            RosMsgKind::F64 => Self::F64(make_publisher::<Float64>(node, topic_name)?),
            RosMsgKind::F32 => Self::F32(make_publisher::<Float32>(node, topic_name)?),
            RosMsgKind::I64 => Self::I64(make_publisher::<Int64>(node, topic_name)?),
            RosMsgKind::I32 => Self::I32(make_publisher::<Int32>(node, topic_name)?),
            RosMsgKind::U64 => Self::U64(make_publisher::<UInt64>(node, topic_name)?),
            RosMsgKind::U32 => Self::U32(make_publisher::<UInt32>(node, topic_name)?),
            RosMsgKind::Bool => Self::Bool(make_publisher::<Bool>(node, topic_name)?),
            RosMsgKind::Str => Self::Str(make_publisher::<RosString>(node, topic_name)?),
            RosMsgKind::Json => Self::Json(make_publisher::<RosString>(node, topic_name)?),
        })
    }

    /// Publish a value. A value whose type no longer matches this publisher's
    /// (the key changed type after the first publish) is logged and dropped.
    pub async fn publish(&self, value: &Value) {
        match (self, value) {
            (Self::F64(p), Value::F64(v)) => drop(p.async_publish(Float64 { data: *v }).await),
            (Self::F32(p), Value::F32(v)) => drop(p.async_publish(Float32 { data: *v }).await),
            (Self::I64(p), Value::I64(v)) => drop(p.async_publish(Int64 { data: *v }).await),
            (Self::I32(p), Value::I32(v)) => drop(p.async_publish(Int32 { data: *v }).await),
            (Self::U64(p), Value::U64(v)) => drop(p.async_publish(UInt64 { data: *v }).await),
            (Self::U32(p), Value::U32(v)) => drop(p.async_publish(UInt32 { data: *v }).await),
            (Self::Bool(p), Value::Boolean(v)) => drop(p.async_publish(Bool { data: *v }).await),
            (Self::Str(p), Value::String(s)) => {
                drop(p.async_publish(RosString { data: s.clone() }).await)
            }
            (Self::Json(p), value) => {
                let data = serde_json::to_string(value).unwrap_or_default();
                drop(p.async_publish(RosString { data }).await);
            }
            (_, value) => warn!(
                "value {value:?} does not match this key's established ROS 2 topic type; dropping"
            ),
        }
    }
}

/// Create a topic and publisher of message type `M` on the given topic name.
fn make_publisher<M: MessageType>(
    node: &mut Node,
    topic_name: &str,
) -> Result<Publisher<M>, String> {
    let ros_name =
        Name::parse(topic_name).map_err(|e| format!("invalid topic name '{topic_name}': {e}"))?;
    #[cfg(feature = "dds")]
    let topic = node
        .create_topic(&ros_name, M::message_type_name(), &DEFAULT_PUBLISHER_QOS)
        .map_err(|e| format!("failed to create topic {topic_name}: {e:?}"))?;
    // The Zenoh backend's `create_topic` is infallible (returns `Topic`).
    #[cfg(feature = "zenoh")]
    let topic = node.create_topic(
        &ros_name,
        M::message_type_name(),
        &QosProfile::publisher_default(),
    );
    node.create_publisher::<M>(&topic, None)
        .map_err(|e| format!("failed to create publisher for {topic_name}: {e:?}"))
}

/// A runtime-typed key publisher: publishes a key's value as a **registered ROS
/// message** (e.g. `hri_msgs/Expression`) rather than a `std_msgs` scalar.
///
/// The message type is not a compile-time struct — it is looked up by ROS name
/// in the shared registry, so any registered message works. Each value is
/// encoded against that message's runtime `low::Type` through the same CDR
/// codec the service and action planes use ([`arora_msgs_ros2::cdr`]) and
/// written to a raw publisher. This is how a device key rides a typed ROS4HRI
/// message end to end.
pub struct TypedKeyPublisher {
    publisher: RawPublisher,
    /// The message's runtime type description; the value is encoded against it.
    message_type: low::Type,
    /// The registry holding the message type's dependency graph, needed to
    /// encode nested types.
    registry: Arc<Ros2Registry>,
    /// The ROS message name, for diagnostics.
    ros_type: String,
}

impl TypedKeyPublisher {
    /// Encode `value` as this publisher's ROS message and publish it. A value
    /// that does not fit the message's type is logged and dropped — the same
    /// non-fatal contract as [`KeyPublisher::publish`].
    pub async fn publish(&self, value: &Value) {
        match cdr::encode(&self.message_type, self.registry.types(), value) {
            Ok(bytes) => {
                let _ = self.publisher.async_publish(&bytes).await;
            }
            Err(e) => warn!(
                "could not encode a value as '{}' ({} bytes of type description): {e}",
                self.ros_type,
                self.message_type.name.len()
            ),
        }
    }
}

/// Create a raw publisher for `ros_type` (a registered ROS message name, e.g.
/// `hri_msgs/Expression`) on `topic_name`. The topic name is used verbatim, so
/// a caller may pass an absolute ROS name (e.g. `/robot_face/expression`) to
/// escape the `/{namespace}/keys/…` convention.
fn make_typed_publisher(
    node: &mut Node,
    topic_name: &str,
    ros_type: &str,
    registry: Arc<Ros2Registry>,
) -> Result<TypedKeyPublisher, String> {
    let message_type = registry
        .get_by_name(ros_type)
        .ok_or_else(|| format!("unknown ROS message type '{ros_type}'"))?
        .clone();
    let (package, type_name) = package_and_type(ros_type)
        .ok_or_else(|| format!("malformed ROS message name '{ros_type}'"))?;
    let ros_name =
        Name::parse(topic_name).map_err(|e| format!("invalid topic name '{topic_name}': {e}"))?;
    let message_type_name = MessageTypeName::new(package, type_name);
    #[cfg(feature = "dds")]
    let topic = node
        .create_topic(&ros_name, message_type_name, &DEFAULT_PUBLISHER_QOS)
        .map_err(|e| format!("failed to create topic {topic_name}: {e:?}"))?;
    // The Zenoh backend's `create_topic` is infallible (returns `Topic`).
    #[cfg(feature = "zenoh")]
    let topic = node.create_topic(
        &ros_name,
        message_type_name,
        &QosProfile::publisher_default(),
    );
    let publisher = node
        .create_raw_publisher(&topic, None)
        .map_err(|e| format!("failed to create raw publisher for {topic_name}: {e:?}"))?;
    Ok(TypedKeyPublisher {
        publisher,
        message_type,
        registry,
        ros_type: ros_type.to_string(),
    })
}

/// Create a [`TypedKeyPublisher`] for a key's declared ROS message type. `topic`
/// is the topic name (an absolute ROS name, or the `/{namespace}/keys/{path}`
/// default the caller resolves) and `ros_type` the registered message name.
pub fn setup_typed_key_publisher(
    node: &mut Node,
    topic: &str,
    ros_type: &str,
    registry: Arc<Ros2Registry>,
) -> Result<TypedKeyPublisher, String> {
    make_typed_publisher(node, topic, ros_type, registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_name_follows_the_keys_convention() {
        assert_eq!(topic_name("robot", "face/mouth"), "/robot/keys/face/mouth");
        assert_eq!(topic_name("robot", "enabled"), "/robot/keys/enabled");
    }

    #[test]
    fn ros_msg_kind_maps_scalars_to_std_msgs() {
        assert_eq!(ros_msg_kind(&Value::F64(0.5)), RosMsgKind::F64);
        assert_eq!(ros_msg_kind(&Value::F32(0.5)), RosMsgKind::F32);
        assert_eq!(ros_msg_kind(&Value::I64(1)), RosMsgKind::I64);
        assert_eq!(ros_msg_kind(&Value::I32(1)), RosMsgKind::I32);
        assert_eq!(ros_msg_kind(&Value::U64(1)), RosMsgKind::U64);
        assert_eq!(ros_msg_kind(&Value::U32(1)), RosMsgKind::U32);
        assert_eq!(ros_msg_kind(&Value::Boolean(true)), RosMsgKind::Bool);
        assert_eq!(ros_msg_kind(&Value::String("x".into())), RosMsgKind::Str);
    }

    #[test]
    fn ros_msg_kind_falls_back_to_json_for_non_scalars() {
        assert_eq!(
            ros_msg_kind(&Value::ArrayF64(vec![1.0, 2.0])),
            RosMsgKind::Json
        );
        assert_eq!(ros_msg_kind(&Value::Unit), RosMsgKind::Json);
    }

    #[test]
    fn std_msgs_type_names_are_correct() {
        assert_eq!(Float64::MESSAGE_TYPE_STR, "std_msgs/Float64");
        assert_eq!(Bool::MESSAGE_TYPE_STR, "std_msgs/Bool");
        assert_eq!(RosString::MESSAGE_TYPE_STR, "std_msgs/String");
    }

    #[test]
    fn json_fallback_round_trips_a_value() {
        let value = Value::ArrayF64(vec![0.1, 0.2, 0.3]);
        let json = serde_json::to_string(&value).unwrap();
        let back: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value, back);
    }

    /// A typed output resolves its ROS message from the same registry the
    /// bridge builds and encodes a device value against it — the publisher's
    /// path. Proven here on `hri_msgs/Expression`: an Expression value encodes
    /// and decodes byte-for-byte through the shared codec, so a key declared
    /// `hri_msgs/Expression` rides a real ROS4HRI message.
    #[test]
    fn a_typed_output_value_round_trips_as_its_ros_message() {
        use arora_msgs_ros2::{builtin_interfaces, hri_msgs, std_msgs};
        use arora_types::value_serde::bridge::to_value_seeded;
        use arora_types::AroraType;

        let registry = arora_msgs_ros2::registry();
        let message_type = registry
            .get_by_name("hri_msgs/Expression")
            .expect("hri_msgs/Expression is registered")
            .clone();

        let expression = hri_msgs::Expression {
            header: std_msgs::Header {
                stamp: builtin_interfaces::Time { sec: 0, nanosec: 0 },
                frame_id: "face".into(),
            },
            expression: "happy".into(),
            valence: 0.8,
            arousal: 0.2,
            confidence: 1.0,
        };
        let (expr_ty, expr_reg) = <hri_msgs::Expression as AroraType>::arora_type_with_registry();
        let value = to_value_seeded(&expression, &expr_ty, &expr_reg).expect("expression to value");

        let bytes =
            cdr::encode(&message_type, registry.types(), &value).expect("encode as Expression");
        let decoded = cdr::decode(&message_type, registry.types(), &bytes).expect("decode");
        assert_eq!(decoded, value);
    }
}
