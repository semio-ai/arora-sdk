//! Freezing a guest module's exports into discoverable signatures, for those
//! whose types freeze without a registry.
//!
//! A host module declares its functions with pre-frozen signatures
//! ([`ModuleBuilder::described_function`](arora_engine::module::ModuleBuilder::described_function)),
//! so they join the method index directly. A guest (wasm) module instead
//! carries a [`Header`](arora_types::module::low::Header) whose exports type
//! their parameters and return with [`low::TypeRef`](arora_types::module::low::TypeRef)
//! — a bare type id, no version. Freezing a reference to a *record* (a
//! structure, an enumeration) needs a registry to pin its version, which the
//! device builder does not hold. A **primitive** reference resolves
//! intrinsically — its id is one of the well-known primitive ids — and so does
//! an **optional** over a scalar primitive, which freezes as a
//! [`FrozenOption`] over that primitive.
//!
//! This module turns a guest export whose parameters and return are all
//! primitives or optional scalar primitives into a [`frozen::Function`],
//! identical to what a host module declares for the same shape, so
//! `DescribeMethods` lists it. An export with any other type yields `None`: it
//! still dispatches, it is just not discoverable. A record (alone, in an array
//! or in an optional) becomes describable once the builder holds a registry to
//! pin its version; a map or a fixed-length array has no frozen form at all.
//!
//! [`frozen::Function`]: Function

use std::collections::HashMap;

use arora_types::module::low::{ExportFunction, TypeRef};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};
use arora_types::ty;
use uuid::Uuid;

/// The [`PrimitiveKind`] a well-known primitive type id names, or `None` if the
/// id is not a primitive (a structure, enumeration, or other registered type).
fn primitive_kind_of(id: Uuid) -> Option<PrimitiveKind> {
    use PrimitiveKind::*;
    // The well-known primitive ids and their kinds — the scalar primitives and
    // their homogeneous-array forms (see `arora_types::ty`).
    let table = [
        (*ty::UNIT_ID, Unit),
        (*ty::BOOLEAN_ID, Boolean),
        (*ty::U8_ID, U8),
        (*ty::U16_ID, U16),
        (*ty::U32_ID, U32),
        (*ty::U64_ID, U64),
        (*ty::I8_ID, I8),
        (*ty::I16_ID, I16),
        (*ty::I32_ID, I32),
        (*ty::I64_ID, I64),
        (*ty::F32_ID, F32),
        (*ty::F64_ID, F64),
        (*ty::STRING_ID, String),
        (*ty::ARRAY_BOOLEAN_ID, ArrayBoolean),
        (*ty::ARRAY_U8_ID, ArrayU8),
        (*ty::ARRAY_U16_ID, ArrayU16),
        (*ty::ARRAY_U32_ID, ArrayU32),
        (*ty::ARRAY_U64_ID, ArrayU64),
        (*ty::ARRAY_I8_ID, ArrayI8),
        (*ty::ARRAY_I16_ID, ArrayI16),
        (*ty::ARRAY_I32_ID, ArrayI32),
        (*ty::ARRAY_I64_ID, ArrayI64),
        (*ty::ARRAY_F32_ID, ArrayF32),
        (*ty::ARRAY_F64_ID, ArrayF64),
        (*ty::ARRAY_STRING_ID, ArrayString),
    ];
    table.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// The homogeneous-array [`PrimitiveKind`] over a scalar element kind, or
/// `None` for a kind that has no array form (unit, or an already-array kind).
fn array_kind_of(element: PrimitiveKind) -> Option<PrimitiveKind> {
    use PrimitiveKind::*;
    Some(match element {
        Boolean => ArrayBoolean,
        U8 => ArrayU8,
        U16 => ArrayU16,
        U32 => ArrayU32,
        U64 => ArrayU64,
        I8 => ArrayI8,
        I16 => ArrayI16,
        I32 => ArrayI32,
        I64 => ArrayI64,
        F32 => ArrayF32,
        F64 => ArrayF64,
        String => ArrayString,
        _ => return None,
    })
}

/// Freeze a [`TypeRef`] that needs no registry into a [`FrozenTy`], or `None`
/// for one that does.
///
/// - A scalar primitive id freezes to that primitive.
/// - A [`TypeRef::Array`] over a primitive element freezes to the matching
///   array primitive.
/// - A [`TypeRef::Option`] over a scalar primitive other than unit freezes to
///   a [`FrozenOption`] over it — what the `#[module]` macro's record and the
///   module authoring tools declare for the same header reference, so a
///   guest's described signature matches its module's record.
///
/// Everything else is `None`. A record reference (a structure or enumeration
/// id, alone, in an array or in an optional) needs a registry to pin its
/// version. An optional over unit is not a type any module generator emits (the
/// macro rejects it, and a C++ module cannot hold one). An array primitive's id
/// is not an element id: the authoring tools read it as a record reference, so
/// an optional naming one is left with the records. A fixed-length array or a
/// map has no [`FrozenTy`] form.
fn freeze_type_ref(ty: &TypeRef) -> Option<FrozenTy> {
    match ty {
        TypeRef::Scalar { id } => primitive_kind_of(*id).map(FrozenTy::from),
        TypeRef::Array { id } => array_kind_of(primitive_kind_of(*id)?).map(FrozenTy::from),
        TypeRef::Option { id } => {
            let element = primitive_kind_of(*id)
                .filter(|kind| kind.is_scalar() && *kind != PrimitiveKind::Unit)?;
            Some(FrozenTy::FrozenOption(FrozenOption {
                element: Box::new(FrozenTy::from(element)),
            }))
        }
        TypeRef::FixedArray { .. } | TypeRef::Map { .. } => None,
    }
}

/// The frozen signature of a guest export whose parameters and return all
/// freeze without a registry (see [`freeze_type_ref`]), or `None` if any does
/// not. The `Some` form is identical to the signature a host module declares
/// for the same shape, so a consumer reads guest and host signatures
/// uniformly.
pub(crate) fn guest_function_signature(function: &ExportFunction) -> Option<Function> {
    let mut parameters = HashMap::new();
    let mut parameter_ordering = Vec::with_capacity(function.parameters.len());
    for param in &function.parameters {
        parameters.insert(
            param.id,
            Parameter {
                name: param.name.clone(),
                ty: freeze_type_ref(&param.ty)?,
                mutable: param.mutable,
            },
        );
        parameter_ordering.push(param.id);
    }
    Some(Function {
        parameters,
        parameter_ordering,
        return_ty: freeze_type_ref(&function.ret)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option_of(kind: PrimitiveKind) -> FrozenTy {
        FrozenTy::FrozenOption(FrozenOption {
            element: Box::new(FrozenTy::from(kind)),
        })
    }

    #[test]
    fn primitives_and_arrays_of_them_freeze_as_primitives() {
        assert_eq!(
            freeze_type_ref(&TypeRef::Scalar { id: *ty::U64_ID }),
            Some(FrozenTy::from(PrimitiveKind::U64))
        );
        assert_eq!(
            freeze_type_ref(&TypeRef::Array { id: *ty::F32_ID }),
            Some(FrozenTy::from(PrimitiveKind::ArrayF32))
        );
    }

    #[test]
    fn an_optional_scalar_primitive_freezes_as_an_optional() {
        assert_eq!(
            freeze_type_ref(&TypeRef::Option { id: *ty::U64_ID }),
            Some(option_of(PrimitiveKind::U64))
        );
        assert_eq!(
            freeze_type_ref(&TypeRef::Option { id: *ty::STRING_ID }),
            Some(option_of(PrimitiveKind::String))
        );
    }

    /// What needs a registry, or has no frozen form, stays undescribed.
    #[test]
    fn records_optional_unit_or_array_primitive_and_maps_do_not_freeze() {
        let record = Uuid::from_u128(0x276);
        for type_ref in [
            TypeRef::Scalar { id: record },
            TypeRef::Array { id: record },
            TypeRef::Option { id: record },
            TypeRef::Option { id: *ty::UNIT_ID },
            TypeRef::Option {
                id: *ty::ARRAY_U8_ID,
            },
            TypeRef::FixedArray {
                id: *ty::U8_ID,
                len: 4,
            },
            TypeRef::Map {
                key_id: *ty::STRING_ID,
                value_id: *ty::U8_ID,
            },
        ] {
            assert_eq!(freeze_type_ref(&type_ref), None, "{type_ref:?}");
        }
    }
}
