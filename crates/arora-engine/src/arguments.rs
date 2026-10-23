//! The arguments of a call to a guest module, checked against the parameters
//! its header declares before the guest reads them.
//!
//! A guest decodes its arguments by its declared types, and what it does with
//! a value of another type is its own: a C++ guest reads it as the declared
//! type, answering as if nothing were wrong; a Rust guest refuses it. Checking
//! in the engine gives every guest, whatever built it, the same answer: a
//! call naming a parameter the function does not declare, or carrying a value
//! not of its parameter's type, fails with [`DispatchError::Argument`] and
//! reaches no guest.
//!
//! What the check leaves to the guest:
//! - an argument that is not an Arora Buffer, or not a structure, which every
//!   guest refuses as its error;
//! - a function the header does not declare, which the executor answers;
//! - a parameter the call leaves out: a guest takes an absent parameter as
//!   absent, and what that means is the function's;
//! - the fields of a record: a record-typed argument is checked by its
//!   record's id, as the header names only that.
//!
//! [`DispatchError::Argument`]: crate::module::DispatchError::Argument

use std::collections::HashMap;

use arora_buffers::serde_uuid::deserialize;
use arora_buffers::well_formed;
use arora_types::module::low::{ExportSymbol, Header, Parameter, TypeRef};
use arora_types::ty::low::validate_type_ref;
use arora_types::value::Value;
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
    /// The signatures `header` declares.
    pub(crate) fn of(header: &Header) -> Self {
        Self(
            header
                .exports
                .iter()
                .map(|ExportSymbol::Function(function)| {
                    let parameters = function
                        .parameters
                        .iter()
                        .map(|parameter| (parameter.id, parameter.clone()))
                        .collect();
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
        let Value::Structure(call) = deserialize(arg) else {
            return Ok(());
        };
        for argument in &call.fields {
            let Some(parameter) = signature.parameters.get(&argument.id) else {
                return Err(format!(
                    "'{}' has no parameter {}",
                    signature.name, argument.id
                ));
            };
            check_argument(&argument.value, &parameter.ty).map_err(|reason| {
                format!(
                    "the argument '{}' of '{}' is not of its declared type: {reason}",
                    parameter.name, signature.name
                )
            })?;
        }
        Ok(())
    }
}

/// Whether `value` is of `ty`, in any form a caller passes it in and every
/// guest reads:
/// - an optional parameter takes an option of its element type, or the
///   element itself;
/// - an array parameter takes the typed array, or a generic array
///   (`Value::ArrayValue`) whose every element is of the element type; a
///   fixed-length array, as many elements as its length.
fn check_argument(value: &Value, ty: &TypeRef) -> Result<(), String> {
    let checked = match (ty, value) {
        (TypeRef::Option { id }, value) if !matches!(value, Value::Option(_)) => {
            validate_type_ref(value, &TypeRef::Scalar { id: *id })
        }
        (TypeRef::Array { id }, Value::ArrayValue(elements)) => check_elements(elements, *id),
        (TypeRef::FixedArray { id, len }, Value::ArrayValue(elements)) => {
            if elements.len() != *len {
                return Err(format!(
                    "fixed array expected {len} elements, got {}",
                    elements.len()
                ));
            }
            check_elements(elements, *id)
        }
        (ty, value) => validate_type_ref(value, ty),
    };
    checked.map_err(|error| error.message)
}

/// Whether every one of `elements` is of the type `id`.
fn check_elements(elements: &[Value], id: Uuid) -> Result<(), arora_types::value::ConversionError> {
    elements
        .iter()
        .try_for_each(|element| validate_type_ref(element, &TypeRef::Scalar { id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_buffers::serde_uuid::serialize;
    use arora_types::module::low::ExportFunction;
    use arora_types::ty;
    use arora_types::value::{Structure, StructureField};

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
        Signatures::of(&header)
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
}
