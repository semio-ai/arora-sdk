//! The arguments of a call to a guest module, checked against the parameters
//! its header declares before the guest reads them.
//!
//! A guest decodes its arguments by its declared types, and what it does with
//! a value of another type is its own: a C++ guest reads it as absent,
//! answering as if it had not been given; a Rust guest refuses it with its own
//! error. Checking in the engine gives every guest, whatever built it, the
//! same answer: a call naming a parameter the function does not declare, or
//! carrying a value not of its parameter's type, fails with
//! [`DispatchError::Argument`] and reaches no guest.
//!
//! The check reads each argument's type tags where they lie in the buffer,
//! stepping over values with [`value_end`], and decodes nothing: an argument
//! costs the host no more memory than its buffer already takes, however many
//! elements it holds.
//!
//! What passes, in the forms callers send:
//! - `unit` for any parameter: callers pass it for an argument they leave
//!   unset, and a guest reads it as absent or refuses it;
//! - any value for a parameter of the dynamic value types (`KEY_VALUE_ID`,
//!   `ARRAY_VALUE_ID`), which guests take as a `Value`;
//! - a present optional argument as an option or as its element;
//! - an array as its typed array, or as a generic array whose every element
//!   is of the element type; a fixed-length array with exactly its length.
//!   A Rust guest generated from a `module.yaml` reads a generic array of
//!   records; a `#[module]` guest refuses a generic array and a C++ guest
//!   reads it as absent.
//!
//! What the check leaves to the guest:
//! - an argument that is not an Arora Buffer, or not a structure, which every
//!   guest refuses as its error;
//! - a function the header does not declare, which the executor answers;
//! - a parameter the call leaves out, which a guest takes as absent;
//! - the fields of a record: a record-typed argument is checked by its
//!   record's id, as the header names only that.
//!
//! [`DispatchError::Argument`]: crate::module::DispatchError::Argument

use std::collections::HashMap;

use arora_buffers::{
    value_end, well_formed, BUFFER_SIZE_SIZE, TYPE_ARRAY, TYPE_BOOLEAN, TYPE_ENUMERATION, TYPE_F32,
    TYPE_F64, TYPE_I16, TYPE_I32, TYPE_I64, TYPE_I8, TYPE_MAP, TYPE_OPTION, TYPE_STRING,
    TYPE_STRUCTURE, TYPE_U16, TYPE_U32, TYPE_U64, TYPE_U8, TYPE_UNIT, TYPE_UUID, TYPE_VALUE,
};
use arora_types::module::low::{ExportSymbol, Header, Parameter, TypeRef};
use arora_types::ty;
use uuid::Uuid;

/// The functions a guest module's header declares, by id: each one's name
/// and parameters.
#[derive(Debug, Clone, Default)]
pub(crate) struct Signatures(HashMap<Uuid, Signature>);

#[derive(Debug, Clone)]
struct Signature {
    name: String,
    parameters: HashMap<Uuid, Parameter>,
}

impl Signatures {
    /// The signatures `header` declares. Of two parameters under one id, the
    /// first is the parameter, as a generated guest reads it.
    pub(crate) fn of(header: &Header) -> Self {
        Self(
            header
                .exports
                .iter()
                .map(|ExportSymbol::Function(function)| {
                    let mut parameters = HashMap::new();
                    for parameter in &function.parameters {
                        parameters
                            .entry(parameter.id)
                            .or_insert_with(|| parameter.clone());
                    }
                    (
                        function.id,
                        Signature {
                            name: function.name.clone(),
                            parameters,
                        },
                    )
                })
                .collect(),
        )
    }

    /// Whether `arg`, the Arora Buffer of a call to `function`, carries only
    /// parameters the function declares, each of its declared type. The
    /// error names the function, the parameter and what is wrong.
    pub(crate) fn check(&self, function: &Uuid, arg: &[u8]) -> Result<(), String> {
        let Some(signature) = self.0.get(function) else {
            return Ok(());
        };
        if well_formed(arg).is_err() {
            return Ok(());
        }
        let buffer = Buffer(arg);
        // A call: the structure's tag, the function's id, its field count,
        // then each field's id and value.
        let start = BUFFER_SIZE_SIZE;
        if buffer.tag(start) != TYPE_STRUCTURE {
            return Ok(());
        }
        let count = buffer.u32(start + 17);
        let mut at = start + 21;
        for _ in 0..count {
            let field = buffer.id(at);
            let value = at + 16;
            let Some(parameter) = signature.parameters.get(&field) else {
                return Err(format!("'{}' has no parameter {field}", signature.name));
            };
            if buffer.tag(value) != TYPE_UNIT {
                buffer.argument(value, &parameter.ty).map_err(|reason| {
                    format!(
                        "the argument '{}' of '{}' is not of its declared type: {reason}",
                        parameter.name, signature.name
                    )
                })?;
            }
            at = buffer.end(value);
        }
        Ok(())
    }
}

/// A well-formed Arora Buffer, read in place: offsets are into the whole
/// buffer, its size prefix included. Every read stays within it, as
/// [`well_formed`] walked it first.
struct Buffer<'a>(&'a [u8]);

impl Buffer<'_> {
    fn tag(&self, at: usize) -> u8 {
        self.0[at]
    }

    fn u32(&self, at: usize) -> usize {
        u32::from_le_bytes(self.0[at..at + 4].try_into().expect("4 bytes")) as usize
    }

    fn id(&self, at: usize) -> Uuid {
        Uuid::from_slice(&self.0[at..at + 16]).expect("16 bytes")
    }

    fn end(&self, at: usize) -> usize {
        value_end(self.0, at).expect("a well-formed buffer's value")
    }

    /// Whether the value at `at` is of the parameter type `ty`.
    fn argument(&self, at: usize, ty: &TypeRef) -> Result<(), String> {
        match ty {
            TypeRef::Scalar { id } => self.scalar(at, *id),
            TypeRef::Array { id } => self.array(at, *id, None),
            TypeRef::FixedArray { id, len } => self.array(at, *id, Some(*len)),
            TypeRef::Map { .. } => self.expect(at, TYPE_MAP, "a map"),
            TypeRef::Option { id } if self.tag(at) == TYPE_OPTION => match self.0[at + 1] {
                0 => Ok(()),
                _ => self.scalar(at + 2, *id),
            },
            TypeRef::Option { id } => self.scalar(at, *id),
        }
    }

    /// Whether the value at `at` is of the type `id` names.
    fn scalar(&self, at: usize, id: Uuid) -> Result<(), String> {
        if dynamic(&id) {
            return Ok(());
        }
        if let Some((tag, name)) = primitive(&id) {
            return self.expect(at, tag, name);
        }
        if id == *ty::OPTION_ID {
            return self.expect(at, TYPE_OPTION, "an option");
        }
        if let Some(element) = typed_array_element(&id) {
            self.expect(at, TYPE_ARRAY, "an array")?;
            return match self.tag(at + 1) == element {
                true => Ok(()),
                false => Err(format!(
                    "expected an array of {}, got an array of {}",
                    tag_name(element),
                    tag_name(self.tag(at + 1))
                )),
            };
        }
        match self.tag(at) {
            TYPE_STRUCTURE | TYPE_ENUMERATION if self.id(at + 1) == id => Ok(()),
            TYPE_STRUCTURE | TYPE_ENUMERATION => Err(format!(
                "expected a value of the record {id}, got one of {}",
                self.id(at + 1)
            )),
            found => Err(format!(
                "expected a value of the record {id}, got {}",
                tag_name(found)
            )),
        }
    }

    /// Whether the value at `at` is an array of elements of the type `id`,
    /// and of `len` of them when that is given.
    fn array(&self, at: usize, id: Uuid, len: Option<usize>) -> Result<(), String> {
        self.expect(at, TYPE_ARRAY, "an array")?;
        let element = self.tag(at + 1);
        let count = self.u32(at + 2);
        if let Some(len) = len.filter(|len| *len != count) {
            return Err(format!("expected {len} elements, got {count}"));
        }
        if dynamic(&id) {
            return Ok(());
        }
        match element {
            TYPE_VALUE => {
                let mut at = at + 6;
                for _ in 0..count {
                    self.scalar(at, id)?;
                    at = self.end(at);
                }
                Ok(())
            }
            TYPE_STRUCTURE | TYPE_ENUMERATION if self.id(at + 6) == id => Ok(()),
            TYPE_STRUCTURE | TYPE_ENUMERATION => Err(format!(
                "expected an array of the record {id}, got one of {}",
                self.id(at + 6)
            )),
            element => match primitive(&id) {
                Some((tag, _)) if tag == element => Ok(()),
                Some((tag, _)) => Err(format!(
                    "expected an array of {}, got an array of {}",
                    tag_name(tag),
                    tag_name(element)
                )),
                None => Err(format!(
                    "expected an array of the record {id}, got an array of {}",
                    tag_name(element)
                )),
            },
        }
    }

    fn expect(&self, at: usize, tag: u8, name: &str) -> Result<(), String> {
        match self.tag(at) {
            found if found == tag => Ok(()),
            found => Err(format!("expected {name}, got {}", tag_name(found))),
        }
    }
}

/// Whether `id` is a dynamic value type, which a guest takes as a `Value`,
/// whatever it holds.
fn dynamic(id: &Uuid) -> bool {
    *id == *ty::KEY_VALUE_ID || *id == *ty::ARRAY_VALUE_ID
}

/// The buffer tag of the primitive type `id`, and what to call a value of it.
fn primitive(id: &Uuid) -> Option<(u8, &'static str)> {
    [
        (&*ty::UNIT_ID, TYPE_UNIT, "a unit"),
        (&ty::BOOLEAN_ID, TYPE_BOOLEAN, "a bool"),
        (&ty::I8_ID, TYPE_I8, "an i8"),
        (&ty::I16_ID, TYPE_I16, "an i16"),
        (&ty::I32_ID, TYPE_I32, "an i32"),
        (&ty::I64_ID, TYPE_I64, "an i64"),
        (&ty::U8_ID, TYPE_U8, "a u8"),
        (&ty::U16_ID, TYPE_U16, "a u16"),
        (&ty::U32_ID, TYPE_U32, "a u32"),
        (&ty::U64_ID, TYPE_U64, "a u64"),
        (&ty::F32_ID, TYPE_F32, "an f32"),
        (&ty::F64_ID, TYPE_F64, "an f64"),
        (&ty::STRING_ID, TYPE_STRING, "a string"),
        (&ty::UUID_ID, TYPE_UUID, "a uuid"),
    ]
    .into_iter()
    .find(|(primitive, ..)| *primitive == id)
    .map(|(_, tag, name)| (tag, name))
}

/// The element tag of the typed array type `id` (`ARRAY_U32_ID`, …).
fn typed_array_element(id: &Uuid) -> Option<u8> {
    [
        (&*ty::ARRAY_BOOLEAN_ID, TYPE_BOOLEAN),
        (&ty::ARRAY_U8_ID, TYPE_U8),
        (&ty::ARRAY_U16_ID, TYPE_U16),
        (&ty::ARRAY_U32_ID, TYPE_U32),
        (&ty::ARRAY_U64_ID, TYPE_U64),
        (&ty::ARRAY_I8_ID, TYPE_I8),
        (&ty::ARRAY_I16_ID, TYPE_I16),
        (&ty::ARRAY_I32_ID, TYPE_I32),
        (&ty::ARRAY_I64_ID, TYPE_I64),
        (&ty::ARRAY_F32_ID, TYPE_F32),
        (&ty::ARRAY_F64_ID, TYPE_F64),
        (&ty::ARRAY_STRING_ID, TYPE_STRING),
    ]
    .into_iter()
    .find(|(array, _)| *array == id)
    .map(|(_, element)| element)
}

/// What to call a value whose buffer tag is `tag`.
fn tag_name(tag: u8) -> &'static str {
    match tag {
        TYPE_UNIT => "a unit",
        TYPE_BOOLEAN => "a bool",
        TYPE_I8 => "an i8",
        TYPE_I16 => "an i16",
        TYPE_I32 => "an i32",
        TYPE_I64 => "an i64",
        TYPE_U8 => "a u8",
        TYPE_U16 => "a u16",
        TYPE_U32 => "a u32",
        TYPE_U64 => "a u64",
        TYPE_F32 => "an f32",
        TYPE_F64 => "an f64",
        TYPE_STRING => "a string",
        TYPE_UUID => "a uuid",
        TYPE_STRUCTURE => "a structure",
        TYPE_ENUMERATION => "an enumeration",
        TYPE_ARRAY => "an array",
        TYPE_MAP => "a map",
        TYPE_OPTION => "an option",
        TYPE_VALUE => "a value",
        _ => "a value of an unknown type",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_buffers::serde_uuid::serialize;
    use arora_types::module::low::ExportFunction;
    use arora_types::ty::low::validate_type_ref;
    use arora_types::value::{Enumeration, Structure, StructureField, Value};

    // Clear of the well-known type ids (`ty::U16_ID` is 7, …).
    const FUNCTION: Uuid = Uuid::from_u128(0xf1c7_0001);
    const COUNT: Uuid = Uuid::from_u128(0xf1c7_0002);
    const LABEL: Uuid = Uuid::from_u128(0xf1c7_0003);
    const SAMPLES: Uuid = Uuid::from_u128(0xf1c7_0004);
    const CORNER: Uuid = Uuid::from_u128(0xf1c7_0005);
    const POINT: Uuid = Uuid::from_u128(0xf1c7_0006);
    const RECORD: Uuid = Uuid::from_u128(0xf1c7_0007);

    fn parameter(id: Uuid, name: &str, ty: TypeRef) -> Parameter {
        Parameter {
            id,
            name: name.into(),
            ty,
            mutable: false,
            default_value: None,
        }
    }

    /// A function `f(count: u32, label: option<str>, samples: [f64],
    /// corner: [f32; 2], point: Record)`.
    fn signatures() -> Signatures {
        Signatures::of(&header())
    }

    fn header() -> Header {
        let mut header = Header {
            id: Uuid::from_u128(0xf1c7_0100),
            name: "m".into(),
            author: "a".into(),
            description: None,
            license: "Apache-2.0".into(),
            version: arora_types::SemanticVersion {
                major: 1,
                minor: 0,
                patch: 0,
            },
            executor: arora_types::module::low::Executor {
                name: "wasm-component".into(),
                min_version: None,
                max_version: None,
            },
            exports: vec![],
            imports: vec![],
            executable_mime: "application/wasm".into(),
        };
        header.exports.push(ExportSymbol::Function(ExportFunction {
            id: FUNCTION,
            name: "f".into(),
            parameters: vec![
                parameter(COUNT, "count", TypeRef::Scalar { id: *ty::U32_ID }),
                parameter(LABEL, "label", TypeRef::Option { id: *ty::STRING_ID }),
                parameter(SAMPLES, "samples", TypeRef::Array { id: *ty::F64_ID }),
                parameter(
                    CORNER,
                    "corner",
                    TypeRef::FixedArray {
                        id: *ty::F32_ID,
                        len: 2,
                    },
                ),
                parameter(POINT, "point", TypeRef::Scalar { id: RECORD }),
            ],
            ret: TypeRef::Scalar { id: *ty::UNIT_ID },
        }));
        header
    }

    fn call(function: Uuid, fields: Vec<(Uuid, Value)>) -> Box<[u8]> {
        serialize(&Value::Structure(Structure {
            id: function,
            fields: fields
                .into_iter()
                .map(|(id, value)| StructureField {
                    id,
                    value: Box::new(value),
                })
                .collect(),
        }))
    }

    fn record(id: Uuid) -> Value {
        Value::Structure(Structure { id, fields: vec![] })
    }

    #[test]
    fn arguments_of_their_declared_types_pass() {
        let signatures = signatures();
        let whole = call(
            FUNCTION,
            vec![
                (COUNT, Value::U32(3)),
                (
                    LABEL,
                    Value::Option(Some(Box::new(Value::String("a".into())))),
                ),
                (SAMPLES, Value::ArrayF64(vec![1.0, 2.0])),
                (CORNER, Value::ArrayF32(vec![0.0, 1.0])),
                (POINT, record(RECORD)),
            ],
        );
        assert_eq!(signatures.check(&FUNCTION, &whole), Ok(()));
        // An optional argument as the element itself, or absent; a
        // parameter left out.
        for fields in [
            vec![(LABEL, Value::String("a".into()))],
            vec![(LABEL, Value::Option(None))],
            vec![(SAMPLES, Value::ArrayValue(vec![Value::F64(1.0)]))],
            vec![(
                CORNER,
                Value::ArrayValue(vec![Value::F32(0.0), Value::F32(1.0)]),
            )],
            vec![],
        ] {
            assert_eq!(signatures.check(&FUNCTION, &call(FUNCTION, fields)), Ok(()));
        }
    }

    #[test]
    fn an_argument_of_another_type_is_refused_naming_it() {
        let signatures = signatures();
        for (field, value) in [
            (COUNT, Value::String("3".into())),
            (COUNT, Value::F64(3.0)),
            (COUNT, Value::I32(3)),
            (LABEL, Value::U32(1)),
            (LABEL, Value::Option(Some(Box::new(Value::U32(1))))),
            (SAMPLES, Value::ArrayF32(vec![1.0])),
            (SAMPLES, Value::F64(1.0)),
            (CORNER, Value::ArrayF32(vec![0.0])),
            (CORNER, Value::ArrayF64(vec![0.0, 1.0])),
            (
                SAMPLES,
                Value::ArrayValue(vec![Value::F64(1.0), Value::U32(1)]),
            ),
            (CORNER, Value::ArrayValue(vec![Value::F32(0.0)])),
            (
                CORNER,
                Value::ArrayValue(vec![Value::F32(0.0), Value::F64(1.0)]),
            ),
            (POINT, record(Uuid::from_u128(0xf1c7_0008))),
            (POINT, Value::U32(1)),
        ] {
            let refused = signatures
                .check(&FUNCTION, &call(FUNCTION, vec![(field, value.clone())]))
                .expect_err(&format!("{value:?}"));
            assert!(
                refused.contains("is not of its declared type") && refused.contains("'f'"),
                "{value:?}: {refused}"
            );
        }
        let refused = signatures
            .check(
                &FUNCTION,
                &call(FUNCTION, vec![(COUNT, Value::Boolean(true))]),
            )
            .unwrap_err();
        assert!(refused.contains("'count'"), "{refused}");
    }

    #[test]
    fn a_parameter_the_function_does_not_declare_is_refused() {
        let unknown = Uuid::from_u128(0xf1c7_0009);
        let refused = signatures()
            .check(&FUNCTION, &call(FUNCTION, vec![(unknown, Value::U32(1))]))
            .unwrap_err();
        assert_eq!(refused, format!("'f' has no parameter {unknown}"));
    }

    #[test]
    fn what_the_guest_answers_is_left_to_it() {
        let signatures = signatures();
        let wrong = vec![(COUNT, Value::String("3".into()))];
        // A function the header does not declare.
        let other = Uuid::from_u128(0xf1c7_0010);
        assert_eq!(
            signatures.check(&other, &call(other, wrong.clone())),
            Ok(())
        );
        // A buffer that is not an Arora Buffer, or not a structure.
        let mut cut = call(FUNCTION, wrong).into_vec();
        cut.truncate(cut.len() - 1);
        for arg in [&[][..], &[1, 2], &cut, &serialize(&Value::U32(1))] {
            assert_eq!(signatures.check(&FUNCTION, arg), Ok(()), "{arg:?}");
        }
    }

    /// The rule the walk implements, stated on decoded values with
    /// arora-types' own type check: the walk must agree with it on every
    /// value and parameter type.
    fn oracle(value: &Value, ty: &TypeRef) -> bool {
        let scalar = |value: &Value, id: Uuid| {
            dynamic(&id) || validate_type_ref(value, &TypeRef::Scalar { id }).is_ok()
        };
        match (ty, value) {
            (_, Value::Unit) => true,
            (TypeRef::Scalar { id }, value) => scalar(value, *id),
            (TypeRef::Option { id }, Value::Option(None)) => {
                let _ = id;
                true
            }
            (TypeRef::Option { id }, Value::Option(Some(inner))) => scalar(inner, *id),
            (TypeRef::Option { id }, value) => scalar(value, *id),
            (TypeRef::Array { id } | TypeRef::FixedArray { id, .. }, value) => {
                let len = match ty {
                    TypeRef::FixedArray { len, .. } => Some(*len),
                    _ => None,
                };
                let elements = match value {
                    Value::ArrayValue(elements) => Some(elements.len()),
                    _ => None,
                };
                let count = elements.or(match value {
                    Value::ArrayBoolean(v) => Some(v.len()),
                    Value::ArrayU32(v) => Some(v.len()),
                    Value::ArrayF64(v) => Some(v.len()),
                    Value::ArrayString(v) => Some(v.len()),
                    Value::ArrayStructure { elements, .. } => Some(elements.len()),
                    _ => None,
                });
                let Some(count) = count else {
                    return false;
                };
                if len.is_some_and(|len| len != count) {
                    return false;
                }
                if dynamic(id) {
                    return true;
                }
                match value {
                    Value::ArrayValue(elements) => elements.iter().all(|e| scalar(e, *id)),
                    value => validate_type_ref(value, &TypeRef::Array { id: *id }).is_ok(),
                }
            }
            (TypeRef::Map { .. }, value) => matches!(value, Value::KeyValue(_)),
        }
    }

    #[test]
    fn the_walk_agrees_with_the_rule_on_every_value_and_type() {
        let record = || {
            Value::Structure(Structure {
                id: RECORD,
                fields: vec![],
            })
        };
        let values = vec![
            Value::Unit,
            Value::Boolean(true),
            Value::U32(1),
            Value::I32(1),
            Value::U64(1),
            Value::F32(1.0),
            Value::F64(1.0),
            Value::String("s".into()),
            Value::Uuid(RECORD),
            record(),
            Value::Structure(Structure {
                id: Uuid::from_u128(0xf1c7_0050),
                fields: vec![],
            }),
            Value::Enumeration(Enumeration {
                id: RECORD,
                variant_id: Uuid::from_u128(0xf1c7_0051),
                value: Box::new(Value::Unit),
            }),
            Value::Option(None),
            Value::Option(Some(Box::new(Value::U32(1)))),
            Value::Option(Some(Box::new(Value::String("s".into())))),
            Value::ArrayU32(vec![1, 2]),
            Value::ArrayF64(vec![1.0]),
            Value::ArrayBoolean(vec![]),
            Value::ArrayString(vec!["a".into()]),
            Value::ArrayStructure {
                id: RECORD,
                elements: vec![],
            },
            Value::ArrayValue(vec![Value::U32(1), Value::U32(2)]),
            Value::ArrayValue(vec![Value::U32(1), Value::F64(2.0)]),
            Value::ArrayValue(vec![record(), record()]),
            Value::ArrayValue(vec![]),
            Value::KeyValue(Default::default()),
        ];
        let ids = [
            *ty::UNIT_ID,
            *ty::BOOLEAN_ID,
            *ty::U32_ID,
            *ty::I32_ID,
            *ty::F64_ID,
            *ty::STRING_ID,
            *ty::UUID_ID,
            *ty::OPTION_ID,
            *ty::ARRAY_U32_ID,
            *ty::ARRAY_STRING_ID,
            *ty::ARRAY_VALUE_ID,
            *ty::KEY_VALUE_ID,
            RECORD,
        ];
        let mut types = vec![TypeRef::Map {
            key_id: *ty::STRING_ID,
            value_id: *ty::U32_ID,
        }];
        for id in ids {
            types.push(TypeRef::Scalar { id });
            types.push(TypeRef::Option { id });
            types.push(TypeRef::Array { id });
            types.push(TypeRef::FixedArray { id, len: 2 });
        }
        let mut pairs = 0;
        for ty in &types {
            let signatures = signatures_with(ty.clone());
            for value in &values {
                let walked = signatures
                    .check(&FUNCTION, &call(FUNCTION, vec![(COUNT, value.clone())]))
                    .is_ok();
                assert_eq!(walked, oracle(value, ty), "{value:?} as {ty:?}");
                pairs += 1;
            }
        }
        assert_eq!(pairs, values.len() * types.len());
    }

    /// A function `f(count: ty)`.
    fn signatures_with(ty: TypeRef) -> Signatures {
        let mut signatures = signatures();
        let signature = signatures.0.get_mut(&FUNCTION).unwrap();
        signature
            .parameters
            .insert(COUNT, parameter(COUNT, "count", ty));
        signatures
    }

    #[test]
    fn unit_dynamic_values_and_generic_arrays_of_records_pass() {
        let signatures = signatures();
        for fields in [
            vec![(COUNT, Value::Unit)],
            vec![(POINT, Value::Unit)],
            vec![(LABEL, Value::Unit)],
        ] {
            assert_eq!(signatures.check(&FUNCTION, &call(FUNCTION, fields)), Ok(()));
        }
        for ty in [
            TypeRef::Scalar {
                id: *ty::KEY_VALUE_ID,
            },
            TypeRef::Scalar {
                id: *ty::ARRAY_VALUE_ID,
            },
            TypeRef::Array {
                id: *ty::KEY_VALUE_ID,
            },
        ] {
            let signatures = signatures_with(ty.clone());
            for value in [
                Value::U32(1),
                Value::ArrayU32(vec![1]),
                Value::String("s".into()),
            ] {
                let checked =
                    signatures.check(&FUNCTION, &call(FUNCTION, vec![(COUNT, value.clone())]));
                let expected =
                    !matches!(ty, TypeRef::Array { .. }) || matches!(value, Value::ArrayU32(_));
                assert_eq!(checked.is_ok(), expected, "{value:?} as {ty:?}");
            }
        }
        let records = signatures_with(TypeRef::Array { id: RECORD });
        let generic = Value::ArrayValue(vec![record(RECORD), record(RECORD)]);
        assert_eq!(
            records.check(&FUNCTION, &call(FUNCTION, vec![(COUNT, generic)])),
            Ok(())
        );
    }

    /// A function with two parameters under one id reads the first, as a
    /// generated guest does.
    #[test]
    fn of_two_parameters_under_one_id_the_first_is_the_parameter() {
        let mut header = header();
        let ExportSymbol::Function(function) = &mut header.exports[0];
        function.parameters.push(parameter(
            COUNT,
            "count again",
            TypeRef::Scalar { id: *ty::STRING_ID },
        ));
        let signatures = Signatures::of(&header);
        let u32 = call(FUNCTION, vec![(COUNT, Value::U32(1))]);
        assert_eq!(signatures.check(&FUNCTION, &u32), Ok(()));
    }

    /// Checking a 20 MB generic array of twenty million units, refused at its
    /// first element, allocates no more than the refusal's message on the
    /// checking thread: the check decodes nothing.
    #[test]
    fn checking_a_large_argument_decodes_nothing() {
        let signatures = signatures();
        let mut writer = arora_buffers::BufferWriter::new();
        writer.begin_structure(FUNCTION.as_bytes(), 1);
        writer.add_structure_field(SAMPLES.as_bytes());
        writer.add_array_primitive(arora_buffers::TYPE_VALUE, 20_000_000);
        for _ in 0..20_000_000 {
            writer.add_unit();
        }
        let arg = writer.finalize();
        let before = counting::allocated();
        let refused = signatures.check(&FUNCTION, &arg);
        let allocated = counting::allocated() - before;
        assert!(refused.is_err());
        assert!(allocated < 4096, "{allocated} bytes allocated: {refused:?}");
        let ok = signatures.check(&FUNCTION, &call(FUNCTION, vec![(COUNT, Value::U32(1))]));
        assert_eq!(ok, Ok(()));
    }

    /// The bytes this thread allocated, counted by the test binary's
    /// allocator.
    mod counting {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        thread_local! {
            static ALLOCATED: Cell<usize> = const { Cell::new(0) };
        }

        struct Counting;

        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                let _ = ALLOCATED.try_with(|n| n.set(n.get() + layout.size()));
                unsafe { System.alloc(layout) }
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                unsafe { System.dealloc(ptr, layout) }
            }
        }

        #[global_allocator]
        static ALLOCATOR: Counting = Counting;

        pub(super) fn allocated() -> usize {
            ALLOCATED.with(Cell::get)
        }
    }
}
