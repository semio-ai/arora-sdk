//! Exposing a device's callable module methods as ROS 2 services.
//!
//! The topic plane ([`conversions`](crate::conversions)) mirrors the device's
//! *keys*; this plane mirrors its *methods*. The bridge asks the runtime for
//! every method's signature ([`arora_bridge::BridgeOp::DescribeMethods`]) and,
//! for each one whose parameter and return types are representable in ROS 2,
//! stands up one service at `/{namespace}/methods/{name}`. Method discovery is
//! automatic — like outbound topics, a device's methods are its own surface, so
//! nothing has to be declared.
//!
//! A method maps onto a `.srv` the way ROS already models one: the parameter
//! list is the **request** message (one field per parameter) and the return
//! value is the **response**. The request/response messages are synthesised as
//! [`low::Type`]s and driven through the shared CDR codec
//! ([`arora_msgs_ros2::cdr`]) against [`arora_msgs_ros2::registry`] — real ROS 2
//! message types on the wire, never an ad-hoc encoding. A request decodes to the
//! call arguments; the [`CallResult`] encodes back as the response.

use arora_bridge::MethodSignature;
use arora_msgs_ros2::{cdr, ros2_representable, Ros2Registry};
use arora_types::call::{Call, CallResult};
use arora_types::gen_uuid_from_str;
use arora_types::module::low::TypeRef;
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::ty::{self, low};
use arora_types::value::{Structure, StructureField, Value};
use arora_types::Uuid;
use ros2_client::{Name, ServiceTypeName};

/// The single field of a synthesised response message wrapping a method's return
/// value. A `unit` return produces an empty response instead (see
/// [`response_type`]).
const RESULT_FIELD: &str = "result";

/// The ROS 2 service name a method is exposed on: `/{namespace}/methods/{name}`.
pub(crate) fn service_name(namespace: &str, method: &str) -> String {
    format!("/{namespace}/methods/{method}")
}

/// A method resolved to a ROS 2 service: the synthesised request/response
/// message types and the call target. Built by [`resolve`]; consumed by the node
/// task, which creates one raw service per entry and, per request, decodes →
/// [`Call`] → encodes the reply.
#[derive(Clone)]
pub(crate) struct MethodService {
    pub name: String,
    pub service_type: ServiceTypeName,
    pub request_type: low::Type,
    pub response_type: low::Type,
    pub module_id: Option<Uuid>,
    pub function_id: Uuid,
}

/// Resolve every method whose signature is representable in ROS 2 to a
/// [`MethodService`]. A method referencing a type ROS 2 cannot carry (a non-ROS
/// scalar, an enum, a map, an unknown record) is skipped — its name is returned
/// alongside so the caller can log the omission (no silent truncation).
pub(crate) fn resolve(
    namespace: &str,
    signatures: &[MethodSignature],
    registry: &Ros2Registry,
) -> (Vec<MethodService>, Vec<String>) {
    let mut services = Vec::new();
    let mut skipped = Vec::new();
    for signature in signatures {
        let request_type = request_type(signature);
        let response_type = response_type(signature);
        let representable = ros2_representable(&request_type, registry.types()).is_ok()
            && ros2_representable(&response_type, registry.types()).is_ok();
        if !representable {
            skipped.push(signature.name.clone());
            continue;
        }
        services.push(MethodService {
            name: service_name(namespace, &signature.name),
            service_type: service_type_name(signature),
            request_type,
            response_type,
            module_id: Some(signature.module_id),
            function_id: signature.id,
        });
    }
    (services, skipped)
}

/// The request message type for a method: a structure with one field per
/// parameter, in declared order. Each field keeps the **parameter's id**, so a
/// decoded request's fields are the [`Call`] arguments verbatim.
fn request_type(signature: &MethodSignature) -> low::Type {
    let function = &signature.function;
    let fields = function.parameter_ordering.iter().filter_map(|id| {
        let parameter = function.parameters.get(id)?;
        Some((
            *id,
            low::StructureField {
                name: parameter.name.clone(),
                type_ref: type_ref_of(&parameter.ty),
            },
        ))
    });
    let name = format!("{}_Request", signature.name);
    low::Type {
        id: gen_uuid_from_str(&name),
        name,
        description: String::new(),
        kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
    }
}

/// The response message type for a method: a structure wrapping the return value
/// in a single `result` field — or an empty structure when the method returns
/// `unit` (which ROS 2 cannot carry as a field, and which needs no data anyway).
fn response_type(signature: &MethodSignature) -> low::Type {
    let ret = &signature.function.return_ty;
    let fields: Vec<(Uuid, low::StructureField)> = if is_unit(ret) {
        Vec::new()
    } else {
        vec![(
            gen_uuid_from_str(RESULT_FIELD),
            low::StructureField {
                name: RESULT_FIELD.to_string(),
                type_ref: type_ref_of(ret),
            },
        )]
    };
    let name = format!("{}_Response", signature.name);
    low::Type {
        id: gen_uuid_from_str(&name),
        name,
        description: String::new(),
        kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
    }
}

/// The synthesised ROS 2 service type for a method: `arora/{Name}`. These are the
/// device's own methods, not standard `.srv`s, so the type is nominal — it names
/// the service on the graph; the messages are described by
/// [`request_type`]/[`response_type`].
fn service_type_name(signature: &MethodSignature) -> ServiceTypeName {
    ServiceTypeName::new("arora", &signature.name)
}

/// Build the [`Call`] for a decoded request value. The request decodes to a
/// [`Value::Structure`] whose fields carry the parameter ids (see
/// [`request_type`]), so its fields are the call arguments unchanged.
pub(crate) fn call_of(service: &MethodService, request: Value) -> Call {
    let args = match request {
        Value::Structure(Structure { fields, .. }) => fields,
        // A non-structure request cannot carry named arguments; call with none.
        _ => Vec::new(),
    };
    Call {
        module_id: service.module_id,
        id: service.function_id,
        args,
    }
}

/// Wrap a call's return value as the response message value: a structure with
/// the single `result` field, or an empty structure for a `unit`-returning
/// method (matching [`response_type`]).
pub(crate) fn response_value(service: &MethodService, result: CallResult) -> Value {
    let fields = if matches!(result.ret, Value::Unit) {
        Vec::new()
    } else {
        vec![StructureField {
            id: gen_uuid_from_str(RESULT_FIELD),
            value: Box::new(result.ret),
        }]
    };
    Value::Structure(Structure {
        id: service.response_type.id,
        fields,
    })
}

/// Encode a response value to CDR bytes for the wire, against the registry.
pub(crate) fn encode_response(
    service: &MethodService,
    value: &Value,
    registry: &Ros2Registry,
) -> Result<Vec<u8>, String> {
    cdr::encode(&service.response_type, registry.types(), value)
        .map_err(|e| format!("encoding response for '{}': {e}", service.name))
}

/// Decode CDR request bytes from the wire to a value, against the registry.
pub(crate) fn decode_request(
    service: &MethodService,
    bytes: &[u8],
    registry: &Ros2Registry,
) -> Result<Value, String> {
    cdr::decode(&service.request_type, registry.types(), bytes)
        .map_err(|e| format!("decoding request for '{}': {e}", service.name))
}

/// A [`Name`] for a service, parsed from its `/{namespace}/methods/{name}` path.
pub(crate) fn parse_name(name: &str) -> Result<Name, String> {
    Name::parse(name).map_err(|e| format!("invalid service name '{name}': {e:?}"))
}

/// The [`TypeRef`] for a frozen type: primitives map to the well-known ids,
/// record references keep their id (the version is not part of the low-level
/// type graph). Mirrors module authoring's private lifting of a frozen type into
/// a module header. Shared with the action plane, which lifts goal parameters
/// the same way.
pub(crate) fn type_ref_of(ty: &FrozenTy) -> TypeRef {
    match ty {
        FrozenTy::Primitive(primitive) => match primitive.kind {
            PrimitiveKind::Unit => TypeRef::Scalar { id: *ty::UNIT_ID },
            PrimitiveKind::Boolean => TypeRef::Scalar {
                id: *ty::BOOLEAN_ID,
            },
            PrimitiveKind::U8 => TypeRef::Scalar { id: *ty::U8_ID },
            PrimitiveKind::U16 => TypeRef::Scalar { id: *ty::U16_ID },
            PrimitiveKind::U32 => TypeRef::Scalar { id: *ty::U32_ID },
            PrimitiveKind::U64 => TypeRef::Scalar { id: *ty::U64_ID },
            PrimitiveKind::I8 => TypeRef::Scalar { id: *ty::I8_ID },
            PrimitiveKind::I16 => TypeRef::Scalar { id: *ty::I16_ID },
            PrimitiveKind::I32 => TypeRef::Scalar { id: *ty::I32_ID },
            PrimitiveKind::I64 => TypeRef::Scalar { id: *ty::I64_ID },
            PrimitiveKind::F32 => TypeRef::Scalar { id: *ty::F32_ID },
            PrimitiveKind::F64 => TypeRef::Scalar { id: *ty::F64_ID },
            PrimitiveKind::String => TypeRef::Scalar { id: *ty::STRING_ID },
            PrimitiveKind::ArrayBoolean => TypeRef::Array {
                id: *ty::BOOLEAN_ID,
            },
            PrimitiveKind::ArrayU8 => TypeRef::Array { id: *ty::U8_ID },
            PrimitiveKind::ArrayU16 => TypeRef::Array { id: *ty::U16_ID },
            PrimitiveKind::ArrayU32 => TypeRef::Array { id: *ty::U32_ID },
            PrimitiveKind::ArrayU64 => TypeRef::Array { id: *ty::U64_ID },
            PrimitiveKind::ArrayI8 => TypeRef::Array { id: *ty::I8_ID },
            PrimitiveKind::ArrayI16 => TypeRef::Array { id: *ty::I16_ID },
            PrimitiveKind::ArrayI32 => TypeRef::Array { id: *ty::I32_ID },
            PrimitiveKind::ArrayI64 => TypeRef::Array { id: *ty::I64_ID },
            PrimitiveKind::ArrayF32 => TypeRef::Array { id: *ty::F32_ID },
            PrimitiveKind::ArrayF64 => TypeRef::Array { id: *ty::F64_ID },
            PrimitiveKind::ArrayString => TypeRef::Array { id: *ty::STRING_ID },
        },
        FrozenTy::FrozenScalar(scalar) => TypeRef::Scalar {
            id: scalar.reference.id,
        },
        FrozenTy::FrozenArray(array) => TypeRef::Array {
            id: array.reference.id,
        },
    }
}

/// Whether a frozen return type is `unit` (no response payload).
fn is_unit(ty: &FrozenTy) -> bool {
    matches!(ty, FrozenTy::Primitive(p) if p.kind == PrimitiveKind::Unit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::record::module::frozen::{Function, Parameter};
    use std::collections::HashMap;

    fn primitive(kind: PrimitiveKind) -> FrozenTy {
        FrozenTy::from(kind)
    }

    /// A frozen `Function` from ordered `(name, type)` params and a return type.
    fn function(params: &[(&str, FrozenTy)], ret: FrozenTy) -> Function {
        let mut parameters = HashMap::new();
        let mut parameter_ordering = Vec::new();
        for (name, ty) in params {
            let id = gen_uuid_from_str(name);
            parameter_ordering.push(id);
            parameters.insert(
                id,
                Parameter {
                    name: (*name).to_string(),
                    ty: ty.clone(),
                    mutable: false,
                },
            );
        }
        Function {
            parameters,
            parameter_ordering,
            return_ty: ret,
        }
    }

    fn signature(name: &str, function: Function) -> MethodSignature {
        MethodSignature {
            module_id: gen_uuid_from_str("module"),
            id: gen_uuid_from_str(name),
            name: name.to_string(),
            function,
        }
    }

    #[test]
    fn service_name_follows_the_methods_convention() {
        assert_eq!(service_name("robot", "speak"), "/robot/methods/speak");
    }

    #[test]
    fn request_type_has_one_field_per_parameter_keyed_by_parameter_id() {
        let sig = signature(
            "add",
            function(
                &[
                    ("a", primitive(PrimitiveKind::F64)),
                    ("b", primitive(PrimitiveKind::F64)),
                ],
                primitive(PrimitiveKind::F64),
            ),
        );
        let request = request_type(&sig);
        let low::TypeKind::Structure(structure) = &request.kind else {
            panic!("request is a structure");
        };
        // Field ids are the parameter ids, in declared order — so a decoded
        // request's fields are the call arguments unchanged.
        let field_ids: Vec<_> = structure.fields.keys().copied().collect();
        assert_eq!(
            field_ids,
            vec![gen_uuid_from_str("a"), gen_uuid_from_str("b")]
        );
    }

    #[test]
    fn unit_return_yields_an_empty_response_type() {
        let sig = signature("reset", function(&[], primitive(PrimitiveKind::Unit)));
        let response = response_type(&sig);
        let low::TypeKind::Structure(structure) = &response.kind else {
            panic!("response is a structure");
        };
        assert!(
            structure.fields.is_empty(),
            "a unit return has no response field"
        );
    }

    #[test]
    fn a_scalar_method_is_representable_and_round_trips_through_cdr() {
        let registry = arora_msgs_ros2::registry();
        let sig = signature(
            "add",
            function(
                &[
                    ("a", primitive(PrimitiveKind::F64)),
                    ("b", primitive(PrimitiveKind::F64)),
                ],
                primitive(PrimitiveKind::F64),
            ),
        );
        let (services, skipped) = resolve("robot", std::slice::from_ref(&sig), &registry);
        assert!(skipped.is_empty(), "an all-f64 method is representable");
        let service = &services[0];
        assert_eq!(service.name, "/robot/methods/add");

        // A request value (a = 2.0, b = 40.0), fields keyed by parameter id.
        let request = Value::Structure(Structure {
            id: service.request_type.id,
            fields: vec![
                StructureField {
                    id: gen_uuid_from_str("a"),
                    value: Box::new(Value::F64(2.0)),
                },
                StructureField {
                    id: gen_uuid_from_str("b"),
                    value: Box::new(Value::F64(40.0)),
                },
            ],
        });
        let bytes =
            cdr::encode(&service.request_type, registry.types(), &request).expect("encode request");
        let decoded = decode_request(service, &bytes, &registry).expect("decode request");

        // The decoded request becomes the call arguments verbatim.
        let call = call_of(service, decoded);
        assert_eq!(call.id, service.function_id);
        assert_eq!(call.module_id, service.module_id);
        assert_eq!(call.args.len(), 2);

        // The response wraps a returned f64 in `result` and round-trips.
        let response = response_value(
            service,
            CallResult {
                ret: Value::F64(42.0),
                mutated: vec![],
            },
        );
        let response_bytes =
            encode_response(service, &response, &registry).expect("encode response");
        let back = cdr::decode(&service.response_type, registry.types(), &response_bytes)
            .expect("decode response");
        assert_eq!(back, response);
    }

    #[test]
    fn a_method_with_a_non_ros_parameter_is_skipped() {
        // `unit` is not a ROS-representable field type, so a method taking one is
        // skipped (its name reported), not silently dropped.
        let sig = signature(
            "weird",
            function(
                &[("x", primitive(PrimitiveKind::Unit))],
                primitive(PrimitiveKind::Unit),
            ),
        );
        let registry = arora_msgs_ros2::registry();
        let (services, skipped) = resolve("robot", std::slice::from_ref(&sig), &registry);
        assert!(services.is_empty());
        assert_eq!(skipped, vec!["weird".to_string()]);
    }
}
