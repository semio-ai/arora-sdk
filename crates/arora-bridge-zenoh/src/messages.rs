//! A request and its answer: each one Arora Buffer holding one value, an
//! array whose first element, a `u8`, says which message it is
//! ([the crate documentation](crate#messages) lists them).
//!
//! A message is built from the request or answer it carries, by value, so its
//! values (a camera frame) are moved into it and copied once, into the
//! buffer. Decoding checks the bytes with [`whole`] first, so bytes that are
//! not one buffer, longer than [`MAX_MESSAGE_LEN`], or one that would decode
//! to more than [`MAX_DECODED_LEN`], are refused, never a panic.

use arora_bridge::BridgeOp;
use arora_buffers::{froto_value, value_end, well_formed_within};
use arora_types::call::{Call, CallResult};
use arora_types::data::{Key, StateChange};
use arora_types::value::{StructureField, Value};
use arora_types::Uuid;

/// The most a message, or a key's value, a peer sends may take, in bytes.
pub(crate) const MAX_MESSAGE_LEN: usize = 64 << 20;

/// The most a message, or a key's value, a peer sends may take decoded.
pub(crate) const MAX_DECODED_LEN: usize = 2 * MAX_MESSAGE_LEN;

/// Check a peer's payload of `len` bytes against [`MAX_MESSAGE_LEN`], before
/// it is made contiguous, which may copy it.
pub(crate) fn check_len(len: usize) -> Result<(), String> {
    if len > MAX_MESSAGE_LEN {
        return Err(format!("a message takes at most {MAX_MESSAGE_LEN} bytes"));
    }
    Ok(())
}

/// Check that `bytes` are one Arora Buffer, nothing after it, of at most
/// [`MAX_MESSAGE_LEN`], that decodes to at most [`MAX_DECODED_LEN`].
pub(crate) fn whole(bytes: &[u8]) -> Result<(), String> {
    check_len(bytes.len())?;
    let declared = bytes
        .get(..4)
        .map(|prefix| u32::from_le_bytes(prefix.try_into().expect("4 bytes")) as usize);
    if declared != Some(bytes.len()) {
        return Err("a message is one buffer, and nothing after it".into());
    }
    well_formed_within(bytes, MAX_DECODED_LEN)?;
    // The size prefix may count bytes the value does not hold.
    if value_end(bytes, 4)? != bytes.len() {
        return Err("a message is one value, and nothing after it".into());
    }
    Ok(())
}

const GET: u8 = 1;
const UPDATE: u8 = 2;
const CALL: u8 = 3;
const LIST_KEYS: u8 = 4;
const DESCRIBE_METHODS: u8 = 5;
const SUBSCRIBED: u8 = 6;
const OK: u8 = 64;
const ERR: u8 = 65;

/// What a client asks of a device.
#[derive(Debug)]
pub(crate) enum Request {
    Op(BridgeOp),
    /// The client's subscribers are declared: answer once the device
    /// publishes to them.
    Subscribed,
}

pub(crate) fn encode_request(request: Request) -> Result<Vec<u8>, String> {
    let items = match request {
        Request::Op(BridgeOp::Get(keys)) => vec![Value::U8(GET), paths(keys)],
        Request::Op(BridgeOp::Update(change)) => {
            let (set, values): (Vec<String>, Vec<Value>) = change
                .set
                .into_iter()
                .map(|(key, value)| (key.path, Value::Option(value.map(Box::new))))
                .unzip();
            vec![
                Value::U8(UPDATE),
                Value::ArrayString(set),
                Value::ArrayValue(values),
                paths(change.unset),
            ]
        }
        Request::Op(BridgeOp::Call(call)) => {
            let mut items = vec![
                Value::U8(CALL),
                Value::Option(call.module_id.map(|id| Box::new(Value::Uuid(id)))),
                Value::Uuid(call.id),
            ];
            items.extend(fields(call.args));
            items
        }
        Request::Op(BridgeOp::ListKeys { prefix }) => {
            vec![Value::U8(LIST_KEYS), optional_string(prefix)]
        }
        Request::Op(BridgeOp::DescribeMethods { prefix }) => {
            vec![Value::U8(DESCRIBE_METHODS), optional_string(prefix)]
        }
        #[allow(deprecated)]
        Request::Op(BridgeOp::ListMethods { .. }) => {
            return Err("list_methods is not carried: use describe_methods".into())
        }
        Request::Subscribed => vec![Value::U8(SUBSCRIBED)],
    };
    Ok(froto_value::serialize(&Value::ArrayValue(items)).into_vec())
}

pub(crate) fn decode_request(bytes: &[u8]) -> Result<Request, String> {
    let (kind, mut items) = open(bytes)?;
    let mut next = |what: &str| {
        items
            .next()
            .ok_or_else(|| format!("a request carries {what}"))
    };
    let request = match kind {
        GET => Request::Op(BridgeOp::Get(
            strings(next("its keys' paths")?)?
                .into_iter()
                .map(Key::from)
                .collect(),
        )),
        UPDATE => {
            let set = strings(next("the paths of the keys set")?)?;
            let Value::ArrayValue(values) = next("the values set")? else {
                return Err("a change's values are an array of values".into());
            };
            if values.len() != set.len() {
                return Err("a change has a value per key set".into());
            }
            let mut change = StateChange::new();
            for (path, value) in set.into_iter().zip(values) {
                let Value::Option(value) = value else {
                    return Err("a key's value is an option".into());
                };
                change
                    .set
                    .insert(Key::from(path), value.map(|value| *value));
            }
            for path in strings(next("the paths of the keys unset")?)? {
                change.unset.insert(Key::from(path));
            }
            Request::Op(BridgeOp::Update(change))
        }
        CALL => {
            let module_id = match next("the module id")? {
                Value::Option(None) => None,
                Value::Option(Some(id)) => Some(uuid(*id)?),
                _ => return Err("the module id is an option".into()),
            };
            let id = uuid(next("the function id")?)?;
            let args = take_fields(next("the arguments' ids")?, next("the arguments")?)?;
            Request::Op(BridgeOp::Call(Call {
                module_id,
                id,
                args,
            }))
        }
        LIST_KEYS => Request::Op(BridgeOp::ListKeys {
            prefix: take_optional_string(next("a prefix")?)?,
        }),
        DESCRIBE_METHODS => Request::Op(BridgeOp::DescribeMethods {
            prefix: take_optional_string(next("a prefix")?)?,
        }),
        SUBSCRIBED => Request::Subscribed,
        other => return Err(format!("no request is of kind {other}")),
    };
    if items.next().is_some() {
        return Err("a request carries nothing more".into());
    }
    Ok(request)
}

pub(crate) fn encode_answer(answer: Result<CallResult, String>) -> Vec<u8> {
    let items = match answer {
        Ok(result) => {
            let mut items = vec![Value::U8(OK), result.ret];
            items.extend(fields(result.mutated));
            items
        }
        Err(reason) => vec![Value::U8(ERR), Value::String(reason)],
    };
    froto_value::serialize(&Value::ArrayValue(items)).into_vec()
}

/// The answer `bytes` carry: the device's result or its reason, or why the
/// bytes are no answer.
pub(crate) fn decode_answer(bytes: &[u8]) -> Result<CallResult, String> {
    let (kind, mut items) = open(bytes)?;
    let mut next = |what: &str| {
        items
            .next()
            .ok_or_else(|| format!("an answer carries {what}"))
    };
    match kind {
        OK => {
            let ret = next("its return value")?;
            let mutated = take_fields(
                next("the mutated parameters' ids")?,
                next("the mutated parameters")?,
            )?;
            Ok(CallResult { ret, mutated })
        }
        ERR => match next("its reason")? {
            Value::String(reason) => Err(reason),
            _ => Err("an error's reason is a string".into()),
        },
        other => Err(format!("no answer is of kind {other}")),
    }
}

/// The kind of the message `bytes` hold, and its other elements.
fn open(bytes: &[u8]) -> Result<(u8, std::vec::IntoIter<Value>), String> {
    whole(bytes)?;
    let Value::ArrayValue(items) = froto_value::deserialize(bytes) else {
        return Err("a message is an array of values".into());
    };
    let mut items = items.into_iter();
    match items.next() {
        Some(Value::U8(kind)) => Ok((kind, items)),
        _ => Err("a message starts with its kind, a u8".into()),
    }
}

fn paths(keys: impl IntoIterator<Item = Key>) -> Value {
    Value::ArrayString(keys.into_iter().map(|key| key.path).collect())
}

fn optional_string(text: Option<String>) -> Value {
    Value::Option(text.map(|text| Box::new(Value::String(text))))
}

/// Fields as two elements: their ids, and their values in the same order.
fn fields(fields: Vec<StructureField>) -> [Value; 2] {
    let (ids, values) = fields
        .into_iter()
        .map(|field| (Value::Uuid(field.id), *field.value))
        .unzip();
    [Value::ArrayValue(ids), Value::ArrayValue(values)]
}

fn take_fields(ids: Value, values: Value) -> Result<Vec<StructureField>, String> {
    let (Value::ArrayValue(ids), Value::ArrayValue(values)) = (ids, values) else {
        return Err("fields are an array of ids and an array of values".into());
    };
    if ids.len() != values.len() {
        return Err("fields have a value per id".into());
    }
    ids.into_iter()
        .zip(values)
        .map(|(id, value)| {
            Ok(StructureField {
                id: uuid(id)?,
                value: Box::new(value),
            })
        })
        .collect()
}

fn strings(value: Value) -> Result<Vec<String>, String> {
    match value {
        Value::ArrayString(strings) => Ok(strings),
        _ => Err("paths are an array of strings".into()),
    }
}

fn take_optional_string(value: Value) -> Result<Option<String>, String> {
    match value {
        Value::Option(None) => Ok(None),
        Value::Option(Some(text)) => match *text {
            Value::String(text) => Ok(Some(text)),
            _ => Err("a prefix is a string".into()),
        },
        _ => Err("a prefix is an option".into()),
    }
}

fn uuid(value: Value) -> Result<Uuid, String> {
    match value {
        Value::Uuid(id) => Ok(id),
        _ => Err("an id is a uuid".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requests() -> Vec<BridgeOp> {
        let mut change = StateChange::set("arm/target", Value::F64(0.5));
        change.set.insert(
            Key::from("camera/frame"),
            Some(Value::ArrayU8(vec![7; 1024])),
        );
        change.set.insert(Key::from("arm/hint"), None);
        change.unset.insert(Key::from("old"));
        vec![
            BridgeOp::Get(vec![Key::from("a"), Key::from("b/c")]),
            BridgeOp::Update(change),
            BridgeOp::Call(Call {
                module_id: Some(Uuid::from_u128(1)),
                id: Uuid::from_u128(2),
                args: vec![StructureField {
                    id: Uuid::from_u128(3),
                    value: Box::new(Value::String("x".into())),
                }],
            }),
            BridgeOp::Call(Call {
                module_id: None,
                id: Uuid::from_u128(2),
                args: Vec::new(),
            }),
            BridgeOp::ListKeys { prefix: None },
            BridgeOp::DescribeMethods {
                prefix: Some("math".into()),
            },
        ]
    }

    /// Every request and answer decodes to what was encoded.
    #[test]
    fn every_message_round_trips() {
        for (op, expected) in requests().into_iter().zip(requests()) {
            let bytes = encode_request(Request::Op(op)).expect("encoded");
            let Request::Op(op) = decode_request(&bytes).expect("decoded") else {
                panic!("not an op");
            };
            let same = match (&op, &expected) {
                (BridgeOp::Get(a), BridgeOp::Get(b)) => a == b,
                (BridgeOp::Update(a), BridgeOp::Update(b)) => a == b,
                (BridgeOp::Call(a), BridgeOp::Call(b)) => a == b,
                (BridgeOp::ListKeys { prefix: a }, BridgeOp::ListKeys { prefix: b }) => a == b,
                (
                    BridgeOp::DescribeMethods { prefix: a },
                    BridgeOp::DescribeMethods { prefix: b },
                ) => a == b,
                _ => false,
            };
            assert!(same, "{op:?} is not {expected:?}");
        }
        let bytes = encode_request(Request::Subscribed).unwrap();
        assert!(matches!(decode_request(&bytes), Ok(Request::Subscribed)));

        let result = CallResult {
            ret: Value::ArrayU8(vec![1, 2, 3]),
            mutated: vec![StructureField {
                id: Uuid::from_u128(9),
                value: Box::new(Value::I32(-1)),
            }],
        };
        assert_eq!(
            decode_answer(&encode_answer(Ok(result.clone()))),
            Ok(result)
        );
        assert_eq!(
            decode_answer(&encode_answer(Err("refused".into()))),
            Err("refused".to_string())
        );
    }

    /// Bytes that are not a message, every truncation of one, an answer sent
    /// as a request and a request sent as an answer are refused, never a
    /// panic.
    #[test]
    fn what_is_not_a_message_is_refused() {
        for op in requests() {
            let bytes = encode_request(Request::Op(op)).unwrap();
            for end in 0..bytes.len() {
                assert!(decode_request(&bytes[..end]).is_err());
            }
            assert!(decode_answer(&bytes).is_err());
        }
        let answer = encode_answer(Err("no".into()));
        assert!(decode_request(&answer).is_err());
        let mut trailing = encode_request(Request::Subscribed).unwrap();
        trailing.extend([0; 16]);
        assert!(decode_request(&trailing).is_err(), "bytes after the buffer");
        let counted = (trailing.len() as u32).to_le_bytes();
        trailing[..4].copy_from_slice(&counted);
        assert!(
            decode_request(&trailing).is_err(),
            "bytes the prefix counts, after the value"
        );
        assert!(decode_request(&[1, 2, 3]).is_err());
        assert!(decode_answer(&[]).is_err());
    }
}
