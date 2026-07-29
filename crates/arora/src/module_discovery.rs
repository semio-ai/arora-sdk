//! Freezing a guest module's primitive-typed exports into discoverable
//! signatures.
//!
//! A host module declares its functions with pre-frozen signatures
//! ([`ModuleBuilder::described_function`](arora_engine::module::ModuleBuilder::described_function)),
//! so they join the method index directly. A guest (wasm) module instead
//! carries a [`Header`](arora_types::module::low::Header) whose exports type
//! their parameters and return with [`low::TypeRef`](arora_types::module::low::TypeRef)
//! — a bare type id, no version. Freezing a *non-primitive* reference needs a
//! registry to pin the version, which the device builder does not hold; but a
//! **primitive** reference resolves intrinsically (its id is one of the
//! well-known primitive ids), so a primitive-only signature freezes with no
//! registry at all.
//!
//! This module does exactly that: it turns a guest export whose parameters and
//! return are all primitives into an [`frozen::Function`], byte-identical to
//! what a host module would declare, so `DescribeMethods` lists it. An export
//! with any non-primitive type yields `None` — it still dispatches, it is just
//! not discoverable until the builder gains a registry seam.

use std::collections::HashMap;

use arora_types::module::low::{ExportFunction, TypeRef};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
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

/// Freeze a primitive [`TypeRef`] into a [`FrozenTy`], or `None` for a
/// non-primitive reference (which needs a registry to pin a version). A
/// [`TypeRef::Array`] over a primitive element becomes the matching array
/// primitive.
fn freeze_type_ref(ty: &TypeRef) -> Option<FrozenTy> {
    match ty {
        TypeRef::Scalar { id } => primitive_kind_of(*id).map(FrozenTy::from),
        TypeRef::Array { id } => array_kind_of(primitive_kind_of(*id)?).map(FrozenTy::from),
        // A fixed-length array, map, or option has no primitive form: its
        // element/entry types would each need registry resolution.
        TypeRef::FixedArray { .. } | TypeRef::Map { .. } | TypeRef::Option { .. } => None,
    }
}

/// The frozen signature of a guest export whose parameters and return are all
/// primitives, or `None` if any type is non-primitive. The `Some` form is
/// byte-identical to the signature a host module declares for the same shape,
/// so a consumer reads guest and host signatures uniformly.
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
