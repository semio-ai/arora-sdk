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
use arora_types::module::low::TypeRef;
use arora_types::ty::{
    low, TypeRegistry, BOOLEAN_ID, F32_ID, F64_ID, I16_ID, I32_ID, I64_ID, I8_ID, STRING_ID,
    U16_ID, U32_ID, U64_ID, U8_ID,
};
use arora_types::value::{Structure, StructureField, Type, Value};
use arora_types::Uuid;
use futures::stream::unfold;
use futures::Stream;
use log::warn;
use ros2_client::{MessageTypeName, Name, Node, Publisher, RawPublisher, RawSubscription};
use tokio::time::{sleep, Duration};

use crate::qos::{self, Qos};

use crate::msg_types::{
    Bool, Float32, Float64, Int32, Int64, MessageType, String as RosString, UInt32, UInt64,
};

/// A boxed stream of single-key [`StateChange`]s produced by one subscription.
pub type StateChangeStream = Pin<Box<dyn Stream<Item = StateChange> + Send>>;

/// The ROS 2 topic name a key is exposed on: `/{namespace}/keys/{path}`.
pub fn topic_name(namespace: &str, path: &str) -> String {
    format!("/{namespace}/keys/{path}")
}

/// The topic handle each backend hands back (`rustdds`' own on `dds`, the
/// Zenoh backend's on `zenoh`) — named once so [`topic_for`] has a return type.
#[cfg(feature = "dds")]
type Topic = ros2_client::rustdds::Topic;
#[cfg(feature = "zenoh")]
type Topic = ros2_client::Topic;

/// Create the ROS 2 topic `name` of type `message_type` under `qos`, uniform
/// across the two backends — the DDS one takes `rustdds` policies and can
/// fail, the Zenoh one takes the neutral profile and cannot.
fn topic_for(
    node: &mut Node,
    name: &Name,
    message_type: MessageTypeName,
    qos: Qos,
) -> Result<Topic, String> {
    #[cfg(feature = "dds")]
    {
        node.create_topic(name, message_type, &qos::dds(qos))
            .map_err(|e| format!("{e:?}"))
    }
    #[cfg(feature = "zenoh")]
    {
        Ok(node.create_topic(name, message_type, &qos::zenoh(qos)))
    }
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

/// The JSON size above which a value is too big for the scalar plane.
///
/// RustDDS fragments anything over 1 KB, and a middleware that has to fragment
/// a sample is where large values go wrong: a vizij face publishing its
/// rendered frame as JSON (a ~70 KB PNG becomes ~300 KB of text) grew the
/// process by ~120 MB in twenty seconds, all of it inside the DDS writer, and
/// the same traffic below the threshold cost nothing. The scalar plane is for
/// scalars; anything image-sized belongs on a declared typed output
/// (`sensor_msgs/CompressedImage`), and even there it is worth knowing that it
/// fragments.
pub(crate) const OVERSIZED_JSON_BYTES: usize = 64 * 1024;

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
    qos: Qos,
) -> Result<StateChangeStream, String> {
    let topic = topic_name(namespace, path);
    let path = path.to_string();

    match value_type {
        Type::F64 => setup_typed::<Float64>(node, &topic, path, qos, |m| Value::F64(m.data)),
        Type::F32 => setup_typed::<Float32>(node, &topic, path, qos, |m| Value::F32(m.data)),
        Type::I64 => setup_typed::<Int64>(node, &topic, path, qos, |m| Value::I64(m.data)),
        Type::I32 => setup_typed::<Int32>(node, &topic, path, qos, |m| Value::I32(m.data)),
        Type::U64 => setup_typed::<UInt64>(node, &topic, path, qos, |m| Value::U64(m.data)),
        Type::U32 => setup_typed::<UInt32>(node, &topic, path, qos, |m| Value::U32(m.data)),
        Type::Boolean => setup_typed::<Bool>(node, &topic, path, qos, |m| Value::Boolean(m.data)),
        Type::String => {
            setup_typed::<RosString>(node, &topic, path, qos, |m| Value::String(m.data))
        }
        other => {
            warn!(
                "key '{path}' has unsupported type {other:?}, falling back to a JSON \
                 std_msgs/String topic"
            );
            setup_typed::<RosString>(node, &topic, path, qos, |m| {
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
    qos: Qos,
    convert: impl Fn(M) -> Value + Send + Sync + 'static,
) -> Result<StateChangeStream, String> {
    let ros_name =
        Name::parse(topic_name).map_err(|e| format!("invalid topic name '{topic_name}': {e}"))?;
    let topic = topic_for(node, &ros_name, M::message_type_name(), qos)
        .map_err(|e| format!("failed to create topic {topic_name}: {e}"))?;

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

/// Await the next raw CDR message from a [`RawSubscription`], uniform across the
/// two backends (DDS `async_take` / Zenoh `take_raw`), as full standalone CDR
/// bytes. Any receive error is stringified so the caller handles both backends'
/// error types the same way.
#[cfg(feature = "dds")]
async fn raw_take(sub: &RawSubscription) -> Result<Vec<u8>, String> {
    sub.async_take()
        .await
        .map(|(bytes, _info)| bytes)
        .map_err(|e| format!("{e:?}"))
}
#[cfg(feature = "zenoh")]
async fn raw_take(sub: &RawSubscription) -> Result<Vec<u8>, String> {
    sub.take_raw()
        .await
        .map(|(bytes, _info)| bytes)
        .map_err(|e| format!("{e:?}"))
}

/// Create a **raw** subscription for a key declared with a ROS message type, and
/// return a stream that decodes each received message against that type into a
/// single-key [`StateChange`] — the typed-inbound counterpart of
/// [`setup_typed`], and the sibling of [`setup_typed_key_publisher`]. A device
/// key thereby receives a real typed ROS message (e.g. `hri_msgs/Expression`)
/// rather than a `std_msgs` scalar.
///
/// `topic` is the topic name (an absolute ROS name, or the
/// `/{namespace}/keys/{path}` default the caller resolves) and `ros_type` the
/// registered message name; `path` is the store key the decoded value lands on.
/// The element type behind any [`TypeRef`](arora_types::module::low::TypeRef)
/// variant — the id a field walk descends into.
pub(crate) fn type_ref_id(type_ref: &arora_types::module::low::TypeRef) -> arora_types::Uuid {
    use arora_types::module::low::TypeRef;
    match type_ref {
        TypeRef::Scalar { id }
        | TypeRef::Array { id }
        | TypeRef::FixedArray { id, .. }
        | TypeRef::Option { id } => *id,
        TypeRef::Map { value_id, .. } => *value_id,
    }
}

/// Resolve a dotted field path (`"header.frame_id"`; empty = the whole
/// message) against a decoded value and its runtime type: each segment looks
/// the field up **by name** in the structure type, then descends type and
/// value together.
///
/// A geometry point/vector — a structure of exactly `x`, `y`, `z` numeric
/// fields — coerces to [`Value::ArrayF32`], the store's vec3 form, so a
/// `PointStamped.point` lands directly on a `gaze/target`-style key.
pub(crate) fn extract_route(
    value: &Value,
    ty: &arora_types::ty::low::Type,
    registry: &arora_types::ty::TypeRegistry,
    dotted: &str,
) -> Result<Value, String> {
    use arora_types::ty::low::TypeKind;
    let mut current_value = value;
    // The current position's type — `None` past a primitive (well-known ids
    // are not registry entries), which only matters for further descent or
    // the vec3 coercion, neither of which applies to a primitive.
    let mut current_ty = Some(ty);
    if !dotted.is_empty() {
        for segment in dotted.split('.') {
            let Some(TypeKind::Structure(structure)) = current_ty.map(|ty| &ty.kind) else {
                return Err(format!("'{dotted}': '{segment}' is not inside a structure"));
            };
            let (field_id, field) = structure
                .fields
                .iter()
                .find(|(_, field)| field.name == segment)
                .ok_or_else(|| format!("'{dotted}': no field '{segment}'"))?;
            let Value::Structure(value_structure) = current_value else {
                return Err(format!(
                    "'{dotted}': value at '{segment}' is not a structure"
                ));
            };
            current_value = value_structure
                .fields
                .iter()
                .find(|value_field| value_field.id == *field_id)
                .map(|value_field| value_field.value.as_ref())
                .ok_or_else(|| format!("'{dotted}': decoded value misses '{segment}'"))?;
            current_ty = registry.get(&type_ref_id(&field.type_ref));
        }
    }
    Ok(current_ty
        .and_then(|ty| coerce_xyz(current_value, ty))
        .unwrap_or_else(|| current_value.clone()))
}

/// Whether a type is a geometry point/vector — a structure of exactly `x`,
/// `y`, `z` fields — the shape [`extract_route`] coerces to the store's vec3
/// form ([`Value::ArrayF32`]).
pub(crate) fn xyz_structure(ty: &arora_types::ty::low::Type) -> bool {
    use arora_types::ty::low::TypeKind;
    let TypeKind::Structure(structure) = &ty.kind else {
        return false;
    };
    let names: Vec<&str> = structure
        .fields
        .values()
        .map(|field| field.name.as_str())
        .collect();
    names == ["x", "y", "z"]
}

/// The vec3 form of an `x`/`y`/`z` numeric structure, `None` for any other
/// shape.
fn coerce_xyz(value: &Value, ty: &arora_types::ty::low::Type) -> Option<Value> {
    if !xyz_structure(ty) {
        return None;
    }
    let Value::Structure(value_structure) = value else {
        return None;
    };
    let mut components = Vec::with_capacity(3);
    for field in &value_structure.fields {
        match field.value.as_ref() {
            Value::F32(v) => components.push(*v),
            Value::F64(v) => components.push(*v as f32),
            _ => return None,
        }
    }
    (components.len() == 3).then_some(Value::ArrayF32(components))
}

/// A zero value of a registry message type: every field defaulted,
/// recursively — what a bound result or feedback message starts from before
/// the meaningful fields are set. `Err` on shapes ROS messages do not use.
pub(crate) fn default_value(ty: &low::Type, registry: &TypeRegistry) -> Result<Value, String> {
    let low::TypeKind::Structure(structure) = &ty.kind else {
        return Err(format!("'{}' is not a structure", ty.name));
    };
    let mut fields = Vec::with_capacity(structure.fields.len());
    for (id, field) in &structure.fields {
        let value = default_of_ref(&field.type_ref, registry)
            .map_err(|e| format!("'{}': {e}", field.name))?;
        fields.push(StructureField {
            id: *id,
            value: Box::new(value),
        });
    }
    Ok(Value::Structure(Structure { id: ty.id, fields }))
}

/// The zero value behind one field reference.
fn default_of_ref(type_ref: &TypeRef, registry: &TypeRegistry) -> Result<Value, String> {
    match type_ref {
        TypeRef::Scalar { id } => default_scalar(id, registry),
        TypeRef::Array { id } => default_array(id, 0),
        TypeRef::FixedArray { id, len } => default_array(id, *len),
        other => Err(format!("no default for a {other:?} field")),
    }
}

/// The zero value of a scalar field: the primitive's zero, or a nested
/// message's default.
fn default_scalar(id: &Uuid, registry: &TypeRegistry) -> Result<Value, String> {
    let id = *id;
    if id == *BOOLEAN_ID {
        Ok(Value::Boolean(false))
    } else if id == *U8_ID {
        Ok(Value::U8(0))
    } else if id == *U16_ID {
        Ok(Value::U16(0))
    } else if id == *U32_ID {
        Ok(Value::U32(0))
    } else if id == *U64_ID {
        Ok(Value::U64(0))
    } else if id == *I8_ID {
        Ok(Value::I8(0))
    } else if id == *I16_ID {
        Ok(Value::I16(0))
    } else if id == *I32_ID {
        Ok(Value::I32(0))
    } else if id == *I64_ID {
        Ok(Value::I64(0))
    } else if id == *F32_ID {
        Ok(Value::F32(0.0))
    } else if id == *F64_ID {
        Ok(Value::F64(0.0))
    } else if id == *STRING_ID {
        Ok(Value::String(String::new()))
    } else if let Some(nested) = registry.get(&id) {
        default_value(nested, registry)
    } else {
        Err(format!("unregistered type {id}"))
    }
}

/// The zero value of an array field (`len` zeros for a fixed array).
fn default_array(id: &Uuid, len: usize) -> Result<Value, String> {
    let id = *id;
    if id == *BOOLEAN_ID {
        Ok(Value::ArrayBoolean(vec![false; len]))
    } else if id == *U8_ID {
        Ok(Value::ArrayU8(vec![0; len]))
    } else if id == *U16_ID {
        Ok(Value::ArrayU16(vec![0; len]))
    } else if id == *U32_ID {
        Ok(Value::ArrayU32(vec![0; len]))
    } else if id == *U64_ID {
        Ok(Value::ArrayU64(vec![0; len]))
    } else if id == *I8_ID {
        Ok(Value::ArrayI8(vec![0; len]))
    } else if id == *I16_ID {
        Ok(Value::ArrayI16(vec![0; len]))
    } else if id == *I32_ID {
        Ok(Value::ArrayI32(vec![0; len]))
    } else if id == *I64_ID {
        Ok(Value::ArrayI64(vec![0; len]))
    } else if id == *F32_ID {
        Ok(Value::ArrayF32(vec![0.0; len]))
    } else if id == *F64_ID {
        Ok(Value::ArrayF64(vec![0.0; len]))
    } else if id == *STRING_ID {
        Ok(Value::ArrayString(vec![String::new(); len]))
    } else {
        Err(format!("no default for an array of type {id}"))
    }
}

/// `value` as the primitive `target` names, when the conversion preserves the
/// kind (any integer feeds an integer field, either float a float field).
pub(crate) fn coerce_scalar(value: &Value, target: &Uuid) -> Option<Value> {
    let int = |value: &Value| -> Option<i128> {
        Some(match value {
            Value::U8(v) => *v as i128,
            Value::U16(v) => *v as i128,
            Value::U32(v) => *v as i128,
            Value::U64(v) => *v as i128,
            Value::I8(v) => *v as i128,
            Value::I16(v) => *v as i128,
            Value::I32(v) => *v as i128,
            Value::I64(v) => *v as i128,
            _ => return None,
        })
    };
    let float = |value: &Value| -> Option<f64> {
        Some(match value {
            Value::F32(v) => *v as f64,
            Value::F64(v) => *v,
            _ => return None,
        })
    };
    let target = *target;
    if target == *BOOLEAN_ID {
        matches!(value, Value::Boolean(_)).then(|| value.clone())
    } else if target == *STRING_ID {
        matches!(value, Value::String(_)).then(|| value.clone())
    } else if target == *U8_ID {
        int(value).map(|v| Value::U8(v as u8))
    } else if target == *U16_ID {
        int(value).map(|v| Value::U16(v as u16))
    } else if target == *U32_ID {
        int(value).map(|v| Value::U32(v as u32))
    } else if target == *U64_ID {
        int(value).map(|v| Value::U64(v as u64))
    } else if target == *I8_ID {
        int(value).map(|v| Value::I8(v as i8))
    } else if target == *I16_ID {
        int(value).map(|v| Value::I16(v as i16))
    } else if target == *I32_ID {
        int(value).map(|v| Value::I32(v as i32))
    } else if target == *I64_ID {
        int(value).map(|v| Value::I64(v as i64))
    } else if target == *F32_ID {
        float(value).map(|v| Value::F32(v as f32))
    } else if target == *F64_ID {
        float(value).map(Value::F64)
    } else {
        None
    }
}

/// Place `new` at a dotted field path (`"header.frame_id"`; empty = the whole
/// message) inside a message value under composition — the outbound mirror of
/// [`extract_route`]: each segment looks the field up **by name** in the
/// structure type and descends type and value together, and the leaf takes
/// the value in the field's own form. A scalar field coerces its kind
/// ([`coerce_scalar`]); a geometry point/vector field (an `x`/`y`/`z`
/// structure) takes the store's vec3 form ([`Value::ArrayF32`] or
/// [`Value::ArrayF64`]) as well as a structure of its type; a nested message
/// or an array field takes a value of its type as is. Nothing is written on an
/// error, so a key of the wrong kind leaves the message as it was.
pub(crate) fn place_route(
    value: &mut Value,
    ty: &low::Type,
    registry: &TypeRegistry,
    dotted: &str,
    new: &Value,
) -> Result<(), String> {
    use arora_types::ty::low::TypeKind;
    if dotted.is_empty() {
        return match new {
            Value::Structure(structure) if structure.id == ty.id => {
                *value = new.clone();
                Ok(())
            }
            other => Err(format!(
                "a whole '{}' message takes a structure of its type, not {other}",
                ty.name
            )),
        };
    }
    let mut current_value = value;
    let mut current_ty = ty;
    let mut segments = dotted.split('.').peekable();
    while let Some(segment) = segments.next() {
        let TypeKind::Structure(structure) = &current_ty.kind else {
            return Err(format!("'{dotted}': '{segment}' is not inside a structure"));
        };
        let (field_id, field) = structure
            .fields
            .iter()
            .find(|(_, field)| field.name == segment)
            .ok_or_else(|| format!("'{dotted}': no field '{segment}'"))?;
        let Value::Structure(value_structure) = current_value else {
            return Err(format!(
                "'{dotted}': value at '{segment}' is not a structure"
            ));
        };
        let slot = value_structure
            .fields
            .iter_mut()
            .find(|value_field| value_field.id == *field_id)
            .map(|value_field| value_field.value.as_mut())
            .ok_or_else(|| format!("'{dotted}': message under composition misses '{segment}'"))?;
        if segments.peek().is_some() {
            current_ty = registry.get(&type_ref_id(&field.type_ref)).ok_or_else(|| {
                format!("'{dotted}': '{segment}' is a primitive, not a structure")
            })?;
            current_value = slot;
            continue;
        }
        // The leaf: the field takes `new` in its own form.
        let placed = match &field.type_ref {
            TypeRef::Scalar { id } => match registry.get(id) {
                None => coerce_scalar(new, id),
                Some(nested) => match new {
                    Value::Structure(structure) if structure.id == nested.id => Some(new.clone()),
                    _ => xyz_value(new, nested),
                },
            },
            TypeRef::Array { .. } | TypeRef::FixedArray { .. } => matches!(
                new,
                Value::ArrayBoolean(_)
                    | Value::ArrayU8(_)
                    | Value::ArrayU16(_)
                    | Value::ArrayU32(_)
                    | Value::ArrayU64(_)
                    | Value::ArrayI8(_)
                    | Value::ArrayI16(_)
                    | Value::ArrayI32(_)
                    | Value::ArrayI64(_)
                    | Value::ArrayF32(_)
                    | Value::ArrayF64(_)
                    | Value::ArrayString(_)
                    | Value::ArrayStructure { .. }
            )
            .then(|| new.clone()),
            other => return Err(format!("'{dotted}': no placement into a {other:?} field")),
        };
        return match placed {
            Some(placed) => {
                *slot = placed;
                Ok(())
            }
            None => Err(format!("'{dotted}': {new} does not fit field '{segment}'")),
        };
    }
    Ok(())
}

/// The `x`/`y`/`z` structure `ty` built from the store's vec3 form of a
/// point ([`Value::ArrayF32`] or [`Value::ArrayF64`] of three), each component
/// coerced into its field's primitive; `None` for any other pair.
fn xyz_value(value: &Value, ty: &low::Type) -> Option<Value> {
    use arora_types::ty::low::TypeKind;
    if !xyz_structure(ty) {
        return None;
    }
    let components: Vec<f64> = match value {
        Value::ArrayF32(v) => v.iter().map(|c| *c as f64).collect(),
        Value::ArrayF64(v) => v.clone(),
        _ => return None,
    };
    if components.len() != 3 {
        return None;
    }
    let TypeKind::Structure(structure) = &ty.kind else {
        return None;
    };
    let mut fields = Vec::with_capacity(3);
    for ((field_id, field), component) in structure.fields.iter().zip(components) {
        let TypeRef::Scalar { id } = &field.type_ref else {
            return None;
        };
        fields.push(StructureField {
            id: *field_id,
            value: Box::new(coerce_scalar(&Value::F64(component), id)?),
        });
    }
    Some(Value::Structure(Structure { id: ty.id, fields }))
}

pub fn setup_typed_key_subscriber(
    node: &mut Node,
    topic: &str,
    ros_type: &str,
    path: String,
    routes: Vec<crate::profile::FieldRoute>,
    registry: Arc<Ros2Registry>,
    qos: Qos,
) -> Result<StateChangeStream, String> {
    let message_type = registry
        .get_by_name(ros_type)
        .ok_or_else(|| format!("unknown ROS message type '{ros_type}'"))?
        .clone();
    let (package, type_name) = package_and_type(ros_type)
        .ok_or_else(|| format!("malformed ROS message name '{ros_type}'"))?;
    let ros_name = Name::parse(topic).map_err(|e| format!("invalid topic name '{topic}': {e}"))?;
    let message_type_name = MessageTypeName::new(package, type_name);
    let ros_topic = topic_for(node, &ros_name, message_type_name, qos)
        .map_err(|e| format!("failed to create topic {topic}: {e}"))?;
    let subscription = node
        .create_raw_subscription(&ros_topic, None)
        .map_err(|e| format!("failed to subscribe to {topic}: {e:?}"))?;

    let ros_type = ros_type.to_string();
    let stream = unfold((subscription, 0u32), move |(sub, errors)| {
        let path = path.clone();
        let ros_type = ros_type.clone();
        let message_type = message_type.clone();
        let registry = registry.clone();
        let routes = routes.clone();
        async move {
            let mut errors = errors;
            loop {
                match raw_take(&sub).await {
                    Ok(bytes) => match cdr::decode(&message_type, registry.types(), &bytes) {
                        Ok(value) => {
                            let mut change = StateChange::new();
                            if routes.is_empty() {
                                change.set.insert(Key::from(path.clone()), Some(value));
                            } else {
                                // All routed fields of one message land in one
                                // atomic change; a field that fails to resolve
                                // is logged and skipped, the rest still land.
                                for route in &routes {
                                    match extract_route(
                                        &value,
                                        &message_type,
                                        registry.types(),
                                        &route.field,
                                    ) {
                                        Ok(extracted) => {
                                            change.set.insert(
                                                Key::from(route.key.clone()),
                                                Some(extracted),
                                            );
                                        }
                                        Err(e) => {
                                            warn!("key '{}': {e}", route.key);
                                        }
                                    }
                                }
                                if change.set.is_empty() {
                                    continue;
                                }
                            }
                            return Some((change, (sub, 0)));
                        }
                        // A message that does not decode against its declared
                        // type is dropped (not a receive failure); wait for the
                        // next one rather than terminating the stream.
                        Err(e) => {
                            warn!("key '{path}': failed to decode a '{ros_type}' message: {e}");
                        }
                    },
                    Err(e) => {
                        errors += 1;
                        if errors >= MAX_CONSECUTIVE_ERRORS {
                            warn!(
                                "subscription for key '{path}' failed {MAX_CONSECUTIVE_ERRORS} \
                                 consecutive times, terminating stream"
                            );
                            return None;
                        }
                        let shift = (errors - 1).min(62) as u64;
                        let delay = Duration::from_millis(100u64.saturating_mul(1u64 << shift));
                        warn!("subscription for key '{path}' errored: {e}; retrying");
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
    pub fn create(
        node: &mut Node,
        topic_name: &str,
        sample: &Value,
        qos: Qos,
    ) -> Result<Self, String> {
        Ok(match ros_msg_kind(sample) {
            RosMsgKind::F64 => Self::F64(make_publisher::<Float64>(node, topic_name, qos)?),
            RosMsgKind::F32 => Self::F32(make_publisher::<Float32>(node, topic_name, qos)?),
            RosMsgKind::I64 => Self::I64(make_publisher::<Int64>(node, topic_name, qos)?),
            RosMsgKind::I32 => Self::I32(make_publisher::<Int32>(node, topic_name, qos)?),
            RosMsgKind::U64 => Self::U64(make_publisher::<UInt64>(node, topic_name, qos)?),
            RosMsgKind::U32 => Self::U32(make_publisher::<UInt32>(node, topic_name, qos)?),
            RosMsgKind::Bool => Self::Bool(make_publisher::<Bool>(node, topic_name, qos)?),
            RosMsgKind::Str => Self::Str(make_publisher::<RosString>(node, topic_name, qos)?),
            RosMsgKind::Json => Self::Json(make_publisher::<RosString>(node, topic_name, qos)?),
        })
    }

    /// Publish a value. A value whose type no longer matches this publisher's
    /// (the key changed type after the first publish) is logged and dropped.
    ///
    /// Returns the size of the JSON encoding when the value took the JSON
    /// fallback, and 0 on every other arm — a `std_msgs` scalar has nothing to
    /// inflate. Past [`OVERSIZED_JSON_BYTES`] that size is a problem in itself.
    pub async fn publish(&self, value: &Value) -> usize {
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
                let size = data.len();
                drop(p.async_publish(RosString { data }).await);
                return size;
            }
            (_, value) => warn!(
                "value {value:?} does not match this key's established ROS 2 topic type; dropping"
            ),
        }
        0
    }
}

/// Create a topic and publisher of message type `M` on the given topic name.
fn make_publisher<M: MessageType>(
    node: &mut Node,
    topic_name: &str,
    qos: Qos,
) -> Result<Publisher<M>, String> {
    let ros_name =
        Name::parse(topic_name).map_err(|e| format!("invalid topic name '{topic_name}': {e}"))?;
    let topic = topic_for(node, &ros_name, M::message_type_name(), qos)
        .map_err(|e| format!("failed to create topic {topic_name}: {e}"))?;
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
    qos: Qos,
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
    let topic = topic_for(node, &ros_name, message_type_name, qos)
        .map_err(|e| format!("failed to create topic {topic_name}: {e}"))?;
    // The publisher is keyed on the message's REP-2016 hash. `rmw_zenoh` puts
    // that hash in the data key, and a native subscriber listens on the exact
    // key, so a publisher without the real hash is discoverable and never
    // heard. The registry holds the full type description, which is all the
    // hash needs; a type it cannot hash still publishes, reaching only other
    // `ros2-client` peers, and says so once.
    let publisher = match arora_msgs_ros2::rihs01(&message_type, registry.types()) {
        Ok(hash) => node
            .create_raw_publisher_with_type_hash(&topic, None, &hash)
            .map_err(|e| format!("failed to create raw publisher for {topic_name}: {e:?}"))?,
        Err(e) => {
            warn!(
                "'{ros_type}' has no REP-2016 hash ({e}); {topic_name} publishes with a \
                 placeholder that native rmw_zenoh subscribers do not match"
            );
            node.create_raw_publisher(&topic, None)
                .map_err(|e| format!("failed to create raw publisher for {topic_name}: {e:?}"))?
        }
    };
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
    qos: Qos,
) -> Result<TypedKeyPublisher, String> {
    make_typed_publisher(node, topic, ros_type, registry, qos)
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

    /// The profile fan-out resolves dotted field paths by name against the
    /// runtime type, and an x/y/z structure coerces to the store's vec3 form.
    /// The outbound mirror: routed keys compose a message that encodes as
    /// its ROS type and reads back through the inbound resolver — a string
    /// key fills `std_msgs/String.data`, a vec3 key becomes a point, and the
    /// fields nobody routed keep their defaults.
    #[test]
    fn routes_place_keys_into_a_message_under_composition() {
        let registry = arora_msgs_ros2::registry();
        let types = registry.types();

        let string = registry
            .get_by_name("std_msgs/String")
            .expect("std_msgs/String is registered");
        let mut message = default_value(string, types).expect("a default String");
        place_route(
            &mut message,
            string,
            types,
            "data",
            &Value::String("hello".into()),
        )
        .expect("place data");
        let bytes = cdr::encode(string, types, &message).expect("encode");
        let decoded = cdr::decode(string, types, &bytes).expect("decode");
        assert_eq!(
            extract_route(&decoded, string, types, "data").unwrap(),
            Value::String("hello".into())
        );

        let stamped = registry
            .get_by_name("geometry_msgs/PointStamped")
            .expect("geometry_msgs/PointStamped is registered");
        let mut message = default_value(stamped, types).expect("a default PointStamped");
        place_route(
            &mut message,
            stamped,
            types,
            "point",
            &Value::ArrayF32(vec![0.5, -0.25, 1.0]),
        )
        .expect("place point");
        place_route(
            &mut message,
            stamped,
            types,
            "header.frame_id",
            &Value::String("face".into()),
        )
        .expect("place frame_id");
        let bytes = cdr::encode(stamped, types, &message).expect("encode");
        let decoded = cdr::decode(stamped, types, &bytes).expect("decode");
        assert_eq!(
            extract_route(&decoded, stamped, types, "point").unwrap(),
            Value::ArrayF32(vec![0.5, -0.25, 1.0])
        );
        assert_eq!(
            extract_route(&decoded, stamped, types, "header.frame_id").unwrap(),
            Value::String("face".into())
        );
        assert_eq!(
            extract_route(&decoded, stamped, types, "header.stamp.sec").unwrap(),
            Value::I32(0)
        );
        // A value of the wrong kind is refused and the message untouched.
        assert!(place_route(
            &mut message,
            stamped,
            types,
            "header.frame_id",
            &Value::F64(1.0)
        )
        .is_err());
        assert_eq!(
            extract_route(&message, stamped, types, "header.frame_id").unwrap(),
            Value::String("face".into())
        );
        assert!(place_route(&mut message, stamped, types, "nope", &Value::F64(1.0)).is_err());
        // The empty path takes the whole message, of its own type only.
        assert!(place_route(&mut message, stamped, types, "", &Value::F64(1.0)).is_err());
        let whole = message.clone();
        place_route(&mut message, stamped, types, "", &whole).expect("whole message");
    }

    #[test]
    fn routes_extract_fields_and_coerce_points() {
        use arora_msgs_ros2::{builtin_interfaces, geometry_msgs, std_msgs};
        use arora_types::value_serde::bridge::to_value_seeded;
        use arora_types::AroraType;

        let registry = arora_msgs_ros2::registry();
        let message_type = registry
            .get_by_name("geometry_msgs/PointStamped")
            .expect("geometry_msgs/PointStamped is registered")
            .clone();
        let stamped = geometry_msgs::PointStamped {
            header: std_msgs::Header {
                stamp: builtin_interfaces::Time { sec: 0, nanosec: 0 },
                frame_id: "sellion_link".into(),
            },
            point: geometry_msgs::Point {
                x: 0.5,
                y: -0.25,
                z: 1.0,
            },
        };
        let (ty, reg) = <geometry_msgs::PointStamped as AroraType>::arora_type_with_registry();
        let value = to_value_seeded(&stamped, &ty, &reg).expect("stamped to value");
        let bytes = cdr::encode(&message_type, registry.types(), &value).expect("encode");
        let decoded = cdr::decode(&message_type, registry.types(), &bytes).expect("decode");

        let target = extract_route(&decoded, &message_type, registry.types(), "point")
            .expect("extract point");
        assert_eq!(target, Value::ArrayF32(vec![0.5, -0.25, 1.0]));
        let frame = extract_route(&decoded, &message_type, registry.types(), "header.frame_id")
            .expect("extract frame_id");
        assert_eq!(frame, Value::String("sellion_link".into()));
        // The empty path is the whole message, untouched.
        let whole =
            extract_route(&decoded, &message_type, registry.types(), "").expect("whole message");
        assert_eq!(whole, decoded);
        // A missing field reports, not panics.
        assert!(extract_route(&decoded, &message_type, registry.types(), "nope").is_err());
    }
}
