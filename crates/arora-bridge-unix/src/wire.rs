//! The wire format: each message is one Arora Buffer, back to back on the
//! stream.
//!
//! A buffer starts with its own length (`u32` little-endian, the prefix
//! included), so the stream needs no framing of its own: a reader takes four
//! bytes, then the rest of the buffer they count. The buffer holds one value,
//! an array of values whose first element, a `u8`, says which message it is.
//! The [crate documentation](crate#wire-format) lists every message.
//!
//! Encoding writes through [`BufferWriter`] straight from the borrowed message,
//! so pushing a change copies no value; decoding checks the buffer with
//! [`well_formed_within`] before reading it, so bytes that are not a buffer,
//! or one that would decode to more than [`MAX_DECODED_LEN`], are refused,
//! never a panic.

use std::collections::{HashMap, HashSet};

use arora_bridge::BridgeOp;
use arora_buffers::froto_value::{deserialize, serialize_to_writer};
use arora_buffers::{well_formed_within, BufferWriter, TYPE_STRING, TYPE_VALUE};
use arora_types::call::{Call, CallResult};
use arora_types::data::{Key, StateChange};
use arora_types::value::{StructureField, Value};
use arora_types::Uuid;

/// The largest message either side accepts, its length prefix included. A
/// longer message closes the connection: its sender is not speaking this wire
/// format, or is sending more than one step's worth of state.
pub const MAX_MESSAGE_LEN: usize = 64 << 20;

/// The most memory a message may take decoded, in bytes, about: a message
/// that would take more is refused before it is decoded, so a small one
/// cannot expand into gigabytes (an array of units takes a byte each on the
/// wire and a whole value each decoded).
pub const MAX_DECODED_LEN: usize = 2 * MAX_MESSAGE_LEN;

/// `get`: read keys.
const GET: u8 = 1;
/// `update`: apply a state change.
const UPDATE: u8 = 2;
/// `call`: call a module function.
const CALL: u8 = 3;
/// `list_keys`: the device's keys and their meta.
const LIST_KEYS: u8 = 4;
/// `describe_methods`: the device's functions and their signatures.
const DESCRIBE_METHODS: u8 = 5;
/// `subscribe`: choose the key prefixes this connection is pushed.
const SUBSCRIBE: u8 = 6;
/// `ok`: a request's answer.
const OK: u8 = 64;
/// `err`: a request's refusal.
const ERR: u8 = 65;
/// `changed`: keys the device changed, among the subscribed prefixes.
const CHANGED: u8 = 66;

/// What a client sends: a request, answered by one [`Message::Answer`] under
/// the same id.
#[derive(Debug, Clone)]
pub enum Request {
    /// An operation on the device, as every bridge carries it.
    Op {
        /// The client's id for the request, echoed in the answer.
        id: u32,
        /// The operation. [`BridgeOp::ListMethods`] has no message: encoding it
        /// fails.
        op: BridgeOp,
    },
    /// Choose which changed keys this connection is pushed: those one of
    /// `prefixes` covers, as a store's prefix does
    /// ([`prefix_covers`](arora_types::data::prefix_covers)): the key itself and
    /// the subtree under it, on a `/` boundary. The empty prefix covers every
    /// key, and no prefix none, which is where a connection starts. Answered
    /// with unit.
    Subscribe {
        /// The client's id for the request, echoed in the answer.
        id: u32,
        /// The key prefixes, replacing the connection's previous ones.
        prefixes: Vec<String>,
        /// Among the subscribed keys, those to push only the newest value of
        /// (camera frames, for instance), replacing the previous ones. While
        /// the client reads slower than the device changes them, the values
        /// it has not been sent yet are replaced rather than queued.
        latest: Vec<String>,
    },
}

/// What the device sends.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The answer to the request `id`: the device's result, or its reason for
    /// refusing.
    Answer {
        /// The request's id.
        id: u32,
        /// The result, as [`arora_bridge::BridgeCommand::reply`] gives it.
        result: Result<CallResult, String>,
    },
    /// Keys the device changed, among those the connection subscribed to.
    Changed(StateChange),
}

/// Encode a request. Fails on [`BridgeOp::ListMethods`], which
/// [`BridgeOp::DescribeMethods`] supersedes.
pub fn encode_request(request: &Request) -> Result<Box<[u8]>, String> {
    let mut w = BufferWriter::new();
    match request {
        Request::Subscribe {
            id,
            prefixes,
            latest,
        } => {
            // `latest` is omitted when empty, as a client that has none sends it.
            head(
                &mut w,
                SUBSCRIBE,
                *id,
                if latest.is_empty() { 1 } else { 2 },
            );
            strings(&mut w, prefixes.iter().map(String::as_str), prefixes.len());
            if !latest.is_empty() {
                strings(&mut w, latest.iter().map(String::as_str), latest.len());
            }
        }
        Request::Op { id, op } => match op {
            BridgeOp::Get(keys) => {
                head(&mut w, GET, *id, 1);
                strings(&mut w, keys.iter().map(|k| k.path.as_str()), keys.len());
            }
            BridgeOp::Update(change) => {
                head(&mut w, UPDATE, *id, 3);
                let set: Vec<_> = change
                    .set
                    .iter()
                    .map(|(k, v)| (k.path.as_str(), v.as_ref()))
                    .collect();
                let unset: Vec<_> = change.unset.iter().map(|k| k.path.as_str()).collect();
                change_parts(&mut w, &set, &unset);
            }
            BridgeOp::Call(call) => {
                head(&mut w, CALL, *id, 4);
                match call.module_id {
                    Some(module) => {
                        w.add_option_some();
                        w.add_uuid(module.as_bytes());
                    }
                    None => w.add_option_none(),
                }
                w.add_uuid(call.id.as_bytes());
                fields(&mut w, &call.args);
            }
            BridgeOp::ListKeys { prefix } => {
                head(&mut w, LIST_KEYS, *id, 1);
                optional_string(&mut w, prefix.as_deref());
            }
            BridgeOp::DescribeMethods { prefix } => {
                head(&mut w, DESCRIBE_METHODS, *id, 1);
                optional_string(&mut w, prefix.as_deref());
            }
            #[allow(deprecated)]
            BridgeOp::ListMethods { .. } => {
                return Err("list_methods has no message: describe_methods supersedes it".into())
            }
        },
    }
    Ok(w.finalize())
}

/// Encode a message.
pub fn encode_message(message: &Message) -> Box<[u8]> {
    match message {
        Message::Answer { id, result } => encode_answer(*id, result),
        Message::Changed(change) => {
            let set: Vec<_> = change
                .set
                .iter()
                .map(|(k, v)| (k.path.as_str(), v.as_ref()))
                .collect();
            let unset: Vec<_> = change.unset.iter().map(|k| k.path.as_str()).collect();
            encode_changed(&set, &unset)
        }
    }
}

/// Encode the answer to request `id`.
pub(crate) fn encode_answer(id: u32, result: &Result<CallResult, String>) -> Box<[u8]> {
    let mut w = BufferWriter::new();
    match result {
        Ok(result) => {
            head(&mut w, OK, id, 3);
            serialize_to_writer(&result.ret, &mut w);
            fields(&mut w, &result.mutated);
        }
        Err(message) => {
            head(&mut w, ERR, id, 1);
            w.add_string(message);
        }
    }
    w.finalize()
}

/// Encode a `changed` message: the keys `set` sets, by path, to their value
/// (none setting a key to no value), and the paths `unset` unsets.
pub(crate) fn encode_changed(set: &[(&str, Option<&Value>)], unset: &[&str]) -> Box<[u8]> {
    let mut w = BufferWriter::new();
    w.add_array_primitive(TYPE_VALUE, 4);
    w.add_u8(CHANGED);
    change_parts(&mut w, set, unset);
    w.finalize()
}

/// Start a request or an answer: the kind, the id, and room for `rest` more
/// elements.
fn head(w: &mut BufferWriter, kind: u8, id: u32, rest: u32) {
    w.add_array_primitive(TYPE_VALUE, 2 + rest);
    w.add_u8(kind);
    w.add_u32(id);
}

fn strings<'a>(w: &mut BufferWriter, items: impl Iterator<Item = &'a str>, len: usize) {
    w.add_array_primitive(TYPE_STRING, len as u32);
    for item in items {
        w.add_string_raw(item);
    }
}

fn optional_string(w: &mut BufferWriter, s: Option<&str>) {
    match s {
        Some(s) => {
            w.add_option_some();
            w.add_string(s);
        }
        None => w.add_option_none(),
    }
}

/// A change as three elements: the set keys' paths, their values as options in
/// the same order, the unset keys' paths.
fn change_parts(w: &mut BufferWriter, set: &[(&str, Option<&Value>)], unset: &[&str]) {
    strings(w, set.iter().map(|(path, _)| *path), set.len());
    w.add_array_primitive(TYPE_VALUE, set.len() as u32);
    for (_, value) in set {
        match value {
            Some(value) => {
                w.add_option_some();
                serialize_to_writer(value, w);
            }
            None => w.add_option_none(),
        }
    }
    strings(w, unset.iter().copied(), unset.len());
}

/// Structure fields as two elements: their ids, their values in the same order.
fn fields(w: &mut BufferWriter, fields: &[StructureField]) {
    w.add_array_primitive(TYPE_VALUE, fields.len() as u32);
    for field in fields {
        w.add_uuid(field.id.as_bytes());
    }
    w.add_array_primitive(TYPE_VALUE, fields.len() as u32);
    for field in fields {
        serialize_to_writer(&field.value, w);
    }
}

/// Decode a request, refusing bytes that are not one.
pub fn decode_request(bytes: &[u8]) -> Result<Request, String> {
    let (kind, mut items) = open(bytes)?;
    if !(GET..=SUBSCRIBE).contains(&kind) {
        return Err(format!("no request is of kind {kind}"));
    }
    let id = take_u32(&mut items)?;
    let request = match kind {
        SUBSCRIBE => Request::Subscribe {
            id,
            prefixes: take_strings(&mut items)?,
            latest: if items.len() > 0 {
                take_strings(&mut items)?
            } else {
                Vec::new()
            },
        },
        GET => Request::Op {
            id,
            op: BridgeOp::Get(
                take_strings(&mut items)?
                    .into_iter()
                    .map(Key::from)
                    .collect(),
            ),
        },
        UPDATE => Request::Op {
            id,
            op: BridgeOp::Update(take_state_change(&mut items)?),
        },
        CALL => {
            let module_id = match items.next() {
                Some(Value::Option(None)) => None,
                Some(Value::Option(Some(module))) => match *module {
                    Value::Uuid(module) => Some(module),
                    other => {
                        return Err(format!(
                            "a module id is a uuid, not {}",
                            shape(Some(&other))
                        ))
                    }
                },
                other => {
                    return Err(format!(
                        "a call's module id is an option, not {}",
                        shape(other.as_ref())
                    ))
                }
            };
            let function = take_uuid(&mut items)?;
            Request::Op {
                id,
                op: BridgeOp::Call(Call {
                    module_id,
                    id: function,
                    args: take_fields(&mut items)?,
                }),
            }
        }
        LIST_KEYS => Request::Op {
            id,
            op: BridgeOp::ListKeys {
                prefix: take_optional_string(&mut items)?,
            },
        },
        DESCRIBE_METHODS => Request::Op {
            id,
            op: BridgeOp::DescribeMethods {
                prefix: take_optional_string(&mut items)?,
            },
        },
        _ => unreachable!("a request kind, checked above"),
    };
    done(items)?;
    Ok(request)
}

/// Decode a message, refusing bytes that are not one.
pub fn decode_message(bytes: &[u8]) -> Result<Message, String> {
    let (kind, mut items) = open(bytes)?;
    let message = match kind {
        OK => {
            let id = take_u32(&mut items)?;
            let ret = items.next().ok_or("an answer carries its return value")?;
            Message::Answer {
                id,
                result: Ok(CallResult {
                    ret,
                    mutated: take_fields(&mut items)?,
                }),
            }
        }
        ERR => {
            let id = take_u32(&mut items)?;
            match items.next() {
                Some(Value::String(message)) => Message::Answer {
                    id,
                    result: Err(message),
                },
                other => {
                    return Err(format!(
                        "a refusal carries a string, not {}",
                        shape(other.as_ref())
                    ))
                }
            }
        }
        CHANGED => Message::Changed(take_state_change(&mut items)?),
        other => return Err(format!("no message is of kind {other}")),
    };
    done(items)?;
    Ok(message)
}

/// About how much memory the keys of `change` that `counted` holds take
/// once decoded: what [`MAX_DECODED_LEN`] limits, and what a queue of
/// changes holds.
pub(crate) fn decoded_size(change: &StateChange, counted: impl Fn(&Key) -> bool) -> usize {
    let set = change
        .set
        .iter()
        .filter(|(key, _)| counted(key))
        .map(|(key, value)| decoded_key(key, Some(value)));
    let unset = change
        .unset
        .iter()
        .filter(|key| counted(key))
        .map(|key| decoded_key(key, None));
    set.chain(unset).sum()
}

/// About how much memory `key` takes decoded in a change, set to `value`,
/// or unset (`None`).
pub(crate) fn decoded_key(key: &Key, value: Option<&Option<Value>>) -> usize {
    STRING + key.path.len() + value.map_or(0, |value| VALUE + value.as_ref().map_or(0, decoded))
}

const VALUE: usize = std::mem::size_of::<Value>();
const STRING: usize = std::mem::size_of::<String>();

fn decoded(value: &Value) -> usize {
    VALUE
        + match value {
            Value::String(s) => s.len(),
            Value::ArrayBoolean(a) => a.len(),
            Value::ArrayU8(a) => a.len(),
            Value::ArrayI8(a) => a.len(),
            Value::ArrayU16(a) => a.len() * 2,
            Value::ArrayI16(a) => a.len() * 2,
            Value::ArrayU32(a) => a.len() * 4,
            Value::ArrayI32(a) => a.len() * 4,
            Value::ArrayF32(a) => a.len() * 4,
            Value::ArrayU64(a) => a.len() * 8,
            Value::ArrayI64(a) => a.len() * 8,
            Value::ArrayF64(a) => a.len() * 8,
            Value::ArrayString(a) => a.iter().map(|s| STRING + s.len()).sum(),
            Value::ArrayValue(a) => a.iter().map(decoded).sum(),
            Value::Option(Some(v)) => decoded(v),
            Value::Structure(s) => s.fields.iter().map(|f| 16 + decoded(&f.value)).sum(),
            Value::Enumeration(e) => decoded(&e.value),
            Value::ArrayStructure { elements, .. } => elements
                .iter()
                .map(|e| {
                    VALUE
                        + e.fields
                            .iter()
                            .map(|f| 16 + decoded(&f.value))
                            .sum::<usize>()
                })
                .sum(),
            Value::ArrayEnumeration { elements, .. } => {
                elements.iter().map(|e| 16 + decoded(&e.value)).sum()
            }
            Value::KeyValue(kv) => kv
                .fields
                .values()
                .map(|f| STRING + f.name.len() + 16 + f.value.as_deref().map_or(0, decoded))
                .sum(),
            _ => 0,
        }
}

type Items = std::vec::IntoIter<Value>;

/// Check `bytes` and open the array it holds: its kind and the elements after
/// it.
fn open(bytes: &[u8]) -> Result<(u8, Items), String> {
    well_formed_within(bytes, MAX_DECODED_LEN).map_err(|e| format!("not an Arora Buffer: {e}"))?;
    let Value::ArrayValue(items) = deserialize(bytes) else {
        return Err("a message is an array of values".into());
    };
    let mut items = items.into_iter();
    match items.next() {
        Some(Value::U8(kind)) => Ok((kind, items)),
        other => Err(format!(
            "a message starts with its kind, a u8, not {}",
            shape(other.as_ref())
        )),
    }
}

fn done(mut items: Items) -> Result<(), String> {
    match items.next() {
        None => Ok(()),
        Some(extra) => Err(format!(
            "the message ends with an element too many: {}",
            shape(Some(&extra))
        )),
    }
}

/// What a value is, by its type alone: an error never formats the value
/// itself, which may be megabytes.
fn shape(value: Option<&Value>) -> String {
    value.map_or_else(|| "nothing".into(), |value| value.kind().to_string())
}

fn take_u32(items: &mut Items) -> Result<u32, String> {
    match items.next() {
        Some(Value::U32(id)) => Ok(id),
        other => Err(format!("expected a u32 id, got {}", shape(other.as_ref()))),
    }
}

fn take_uuid(items: &mut Items) -> Result<Uuid, String> {
    match items.next() {
        Some(Value::Uuid(id)) => Ok(id),
        other => Err(format!("expected a uuid, got {}", shape(other.as_ref()))),
    }
}

fn take_strings(items: &mut Items) -> Result<Vec<String>, String> {
    match items.next() {
        Some(Value::ArrayString(strings)) => Ok(strings),
        other => Err(format!(
            "expected an array of strings, got {}",
            shape(other.as_ref())
        )),
    }
}

fn take_values(items: &mut Items) -> Result<Vec<Value>, String> {
    match items.next() {
        Some(Value::ArrayValue(values)) => Ok(values),
        other => Err(format!(
            "expected an array of values, got {}",
            shape(other.as_ref())
        )),
    }
}

fn take_optional_string(items: &mut Items) -> Result<Option<String>, String> {
    match items.next() {
        Some(Value::Option(None)) => Ok(None),
        Some(Value::Option(Some(s))) => match *s {
            Value::String(s) => Ok(Some(s)),
            other => Err(format!("expected a string, got {}", shape(Some(&other)))),
        },
        other => Err(format!(
            "expected an optional string, got {}",
            shape(other.as_ref())
        )),
    }
}

fn take_state_change(items: &mut Items) -> Result<StateChange, String> {
    let paths = take_strings(items)?;
    let values = take_values(items)?;
    if paths.len() != values.len() {
        return Err(format!(
            "a change sets {} keys to {} values",
            paths.len(),
            values.len()
        ));
    }
    let set: HashMap<Key, Option<Value>> = paths
        .into_iter()
        .zip(values)
        .map(|(path, value)| match value {
            Value::Option(value) => Ok((Key::from(path), value.map(|v| *v))),
            other => Err(format!(
                "a set key's value is an option, not {}",
                shape(Some(&other))
            )),
        })
        .collect::<Result<_, String>>()?;
    let unset: HashSet<Key> = take_strings(items)?.into_iter().map(Key::from).collect();
    Ok(StateChange { set, unset })
}

fn take_fields(items: &mut Items) -> Result<Vec<StructureField>, String> {
    let ids = take_values(items)?;
    let values = take_values(items)?;
    if ids.len() != values.len() {
        return Err(format!(
            "{} field ids for {} values",
            ids.len(),
            values.len()
        ));
    }
    ids.into_iter()
        .zip(values)
        .map(|(id, value)| match id {
            Value::Uuid(id) => Ok(StructureField {
                id,
                value: Box::new(value),
            }),
            other => Err(format!("a field id is a uuid, not {}", shape(Some(&other)))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::keyvalue::KeyValue;

    fn sample_values() -> Vec<Value> {
        vec![
            Value::Unit,
            Value::Boolean(true),
            Value::F32(0.25),
            Value::F64(-1.5),
            Value::I64(-7),
            Value::String("héllo".into()),
            Value::ArrayF32((0..25).map(|i| i as f32 * 0.1).collect()),
            Value::ArrayString(vec![]),
            Value::ArrayValue(vec![Value::U8(1), Value::String("x".into())]),
            Value::Option(Some(Box::new(Value::U16(3)))),
            Value::Option(None),
            Value::Uuid(Uuid::new_v4()),
            Value::KeyValue(KeyValue::new()),
        ]
    }

    fn change() -> StateChange {
        let mut change = StateChange::new();
        for (i, value) in sample_values().into_iter().enumerate() {
            change.set.insert(Key::from(format!("k/{i}")), Some(value));
        }
        change.set.insert(Key::from("cleared"), None);
        change.unset.insert(Key::from("gone"));
        change
    }

    fn call() -> Call {
        Call {
            module_id: Some(Uuid::new_v4()),
            id: Uuid::new_v4(),
            args: sample_values()
                .into_iter()
                .map(|value| StructureField {
                    id: Uuid::new_v4(),
                    value: Box::new(value),
                })
                .collect(),
        }
    }

    /// Requests are compared through their debug form: `BridgeOp` has no
    /// `PartialEq`.
    fn round_trip(request: Request) {
        let bytes = encode_request(&request).unwrap();
        let back = decode_request(&bytes).unwrap();
        assert_eq!(format!("{request:?}"), format!("{back:?}"));
    }

    #[test]
    fn every_request_round_trips() {
        round_trip(Request::Subscribe {
            id: 1,
            prefixes: vec!["".into(), "joints/".into()],
            latest: vec![],
        });
        round_trip(Request::Subscribe {
            id: 2,
            prefixes: vec![],
            latest: vec![],
        });
        round_trip(Request::Subscribe {
            id: 2,
            prefixes: vec!["".into()],
            latest: vec!["camera".into()],
        });
        round_trip(Request::Op {
            id: 3,
            op: BridgeOp::Get(vec![Key::from("a"), Key::from("b/c")]),
        });
        // A change of one key: a `HashMap` of several would order its debug
        // form at random.
        round_trip(Request::Op {
            id: 4,
            op: BridgeOp::Update(StateChange::set("head/yaw", Value::F32(0.3))),
        });
        round_trip(Request::Op {
            id: 5,
            op: BridgeOp::Call(call()),
        });
        round_trip(Request::Op {
            id: 6,
            op: BridgeOp::Call(Call {
                module_id: None,
                id: Uuid::new_v4(),
                args: vec![],
            }),
        });
        for prefix in [None, Some("joints".to_string())] {
            round_trip(Request::Op {
                id: 7,
                op: BridgeOp::ListKeys {
                    prefix: prefix.clone(),
                },
            });
            round_trip(Request::Op {
                id: u32::MAX,
                op: BridgeOp::DescribeMethods { prefix },
            });
        }
    }

    #[test]
    fn an_update_of_many_keys_round_trips() {
        let change = change();
        let request = Request::Op {
            id: 9,
            op: BridgeOp::Update(change.clone()),
        };
        let Request::Op {
            op: BridgeOp::Update(back),
            ..
        } = decode_request(&encode_request(&request).unwrap()).unwrap()
        else {
            panic!("an update");
        };
        assert_eq!(back, change);
    }

    #[test]
    fn every_message_round_trips() {
        for message in [
            Message::Answer {
                id: 1,
                result: Ok(CallResult {
                    ret: Value::ArrayValue(sample_values()),
                    mutated: call().args,
                }),
            },
            Message::Answer {
                id: 2,
                result: Ok(CallResult {
                    ret: Value::Unit,
                    mutated: vec![],
                }),
            },
            Message::Answer {
                id: 3,
                result: Err("refused: k".into()),
            },
            Message::Changed(change()),
            Message::Changed(StateChange::new()),
        ] {
            assert_eq!(decode_message(&encode_message(&message)).unwrap(), message);
        }
    }

    #[test]
    fn a_change_holds_the_keys_given() {
        let change = change();
        let set: Vec<_> = change
            .set
            .iter()
            .filter(|(k, _)| k.path.starts_with("k/1"))
            .map(|(k, v)| (k.path.as_str(), v.as_ref()))
            .collect();
        let bytes = encode_changed(&set, &[]);
        let Message::Changed(back) = decode_message(&bytes).unwrap() else {
            panic!("a change");
        };
        let mut paths: Vec<_> = back.set.keys().map(|k| k.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["k/1", "k/10", "k/11", "k/12"]);
        assert!(back.unset.is_empty());
    }

    #[test]
    fn list_methods_has_no_message() {
        #[allow(deprecated)]
        let op = BridgeOp::ListMethods { prefix: None };
        assert!(encode_request(&Request::Op { id: 1, op }).is_err());
    }

    #[test]
    fn a_message_is_not_a_request() {
        let bytes = encode_message(&Message::Changed(change()));
        assert!(decode_request(&bytes).unwrap_err().contains("kind 66"));
    }

    #[test]
    fn bytes_that_are_not_a_message_are_refused() {
        // Not a buffer; a buffer of the wrong shape; an element too many.
        assert!(decode_request(&[1, 2, 3]).is_err());
        assert!(decode_request(&arora_buffers::froto_value::serialize(&Value::F64(1.0))).is_err());
        let extra = Value::ArrayValue(vec![
            Value::U8(LIST_KEYS),
            Value::U32(1),
            Value::Option(None),
            Value::Unit,
        ]);
        let bytes = arora_buffers::froto_value::serialize(&extra);
        assert!(decode_request(&bytes).unwrap_err().contains("too many"));
    }

    /// A message small on the wire that would expand past
    /// [`MAX_DECODED_LEN`] once decoded is refused before it is decoded.
    #[test]
    fn a_message_that_would_decode_too_large_is_refused() {
        let units = 8 << 20;
        let mut w = BufferWriter::new();
        w.add_array_primitive(TYPE_VALUE, 2);
        w.add_u8(LIST_KEYS);
        w.add_array_primitive(TYPE_VALUE, units);
        for _ in 0..units {
            w.add_unit();
        }
        let bytes = w.finalize();
        assert!(
            bytes.len() < MAX_MESSAGE_LEN,
            "{} bytes on the wire",
            bytes.len()
        );
        let error = decode_request(&bytes).unwrap_err();
        assert!(error.contains("decodes to more than"), "{error}");
    }

    /// A change counts what it takes decoded, far more than its bytes for an
    /// array of units.
    #[test]
    fn a_change_counts_what_it_takes_decoded() {
        let mut change = StateChange::new();
        change.set.insert(
            Key::from("units"),
            Some(Value::ArrayValue(vec![Value::Unit; 1 << 18])),
        );
        let bytes = encode_message(&Message::Changed(change.clone()));
        let decoded = decoded_size(&change, |_| true);
        assert!(
            decoded >= 16 * bytes.len(),
            "{decoded} decoded, {} on the wire",
            bytes.len()
        );
    }

    /// An error names a misplaced value by its type, never by its contents,
    /// which may be megabytes.
    #[test]
    fn an_error_names_a_value_by_its_type() {
        let large = || Value::ArrayU8(vec![255; 1 << 20]);
        // Where the kind belongs, and after a whole request.
        let misplaced = Value::ArrayValue(vec![large()]);
        let trailing = Value::ArrayValue(vec![
            Value::U8(LIST_KEYS),
            Value::U32(1),
            Value::Option(None),
            large(),
        ]);
        for wrong in [misplaced, trailing] {
            let bytes = arora_buffers::froto_value::serialize(&wrong);
            let error = decode_request(&bytes).unwrap_err();
            assert!(
                error.len() < 200,
                "{} bytes of error: {}",
                error.len(),
                &error[..200.min(error.len())]
            );
        }
    }

    /// Every truncation and every single-byte corruption of valid messages is
    /// refused or decoded, never a panic.
    #[test]
    fn corrupted_bytes_never_panic() {
        let messages = [
            encode_request(&Request::Op {
                id: 5,
                op: BridgeOp::Call(call()),
            })
            .unwrap(),
            encode_request(&Request::Op {
                id: 9,
                op: BridgeOp::Update(change()),
            })
            .unwrap(),
            encode_message(&Message::Changed(change())),
        ];
        for bytes in &messages {
            for len in 0..bytes.len() {
                let _ = decode_request(&bytes[..len]);
                let _ = decode_message(&bytes[..len]);
            }
            for at in 0..bytes.len() {
                for flip in [0x01u8, 0x80, 0xff] {
                    let mut corrupt = bytes.to_vec();
                    corrupt[at] ^= flip;
                    let _ = decode_request(&corrupt);
                    let _ = decode_message(&corrupt);
                }
            }
        }
    }
}
