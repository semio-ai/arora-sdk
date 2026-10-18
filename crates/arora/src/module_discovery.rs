//! A guest module's exports as the method index describes them: from the
//! module's record when the device holds it ([`check_record`] checks it
//! against the header), else frozen from the header, for those exports whose
//! types freeze without a registry.
//!
//! A host module declares its functions with pre-frozen signatures
//! ([`ModuleBuilder::described_function`](arora_engine::module::ModuleBuilder::described_function)),
//! so they join the method index directly. A guest (wasm) module instead
//! carries a [`Header`](arora_types::module::low::Header) whose exports type
//! their parameters and return with [`low::TypeRef`](arora_types::module::low::TypeRef)
//! — a bare type id, no version. Freezing a reference to a *record* (a
//! structure, an enumeration) needs its version, which the module's record
//! pins and the header does not. A **primitive** reference resolves
//! intrinsically — its id is one of the well-known primitive ids — and so does
//! an **optional** over a scalar primitive, which freezes as a
//! [`FrozenOption`] over that primitive.
//!
//! Without a record, a guest export whose parameters and return are all
//! primitives or optional scalar primitives becomes a [`frozen::Function`],
//! identical to what a host module declares for the same shape, so
//! `DescribeMethods` lists it. An export with any other type yields `None`: it
//! still dispatches, it is just not discoverable. A record (alone, in an array
//! or in an optional) is described from the module's record, which pins its
//! version; a map or a fixed-length array has no frozen form at all.
//!
//! [`frozen::Function`]: Function

use std::collections::HashMap;

use arora_types::module::low::{ExportFunction, ExportSymbol, Header, TypeRef};
use arora_types::record::module::frozen::{self, ExportKind, Function, Parameter};
use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};
use arora_types::record::Version;
use arora_types::SemanticVersion;
use uuid::Uuid;

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
        TypeRef::Scalar { id } => PrimitiveKind::from_id(id).map(FrozenTy::from),
        TypeRef::Array { id } => PrimitiveKind::from_id(id)?.array_of().map(FrozenTy::from),
        TypeRef::Option { id } => {
            let element = PrimitiveKind::from_id(id)
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

/// Whether `record`, a guest module's record, describes `header`: the same
/// exports, each with the same id and name, the same parameters in the same
/// order (ids, names and mutability), and each type naming the id the
/// header's names. `Err` says how they differ, naming the export.
pub(crate) fn check_record(header: &Header, record: &frozen::Module) -> Result<(), String> {
    for export in &header.exports {
        let ExportSymbol::Function(function) = export;
        let what = format!("export '{}' ({})", function.name, function.id);
        let Some(declared) = record.exports.get(&function.id) else {
            return Err(format!("{what} is not in the record"));
        };
        if declared.name != function.name {
            return Err(format!("{what} is named '{}' in the record", declared.name));
        }
        let ExportKind::Function(declared) = &declared.kind;
        let ids: Vec<Uuid> = function.parameters.iter().map(|p| p.id).collect();
        if declared.parameter_ordering != ids {
            return Err(format!(
                "{what} has parameters {ids:?} in the header and {:?} in the record",
                declared.parameter_ordering
            ));
        }
        for parameter in &function.parameters {
            let Some(described) = declared.parameters.get(&parameter.id) else {
                return Err(format!(
                    "{what}: the record orders parameter '{}' ({}) but does not declare it",
                    parameter.name, parameter.id
                ));
            };
            if described.name != parameter.name || described.mutable != parameter.mutable {
                return Err(format!(
                    "{what}: parameter {} is '{}'{} in the header and '{}'{} in the record",
                    parameter.id,
                    parameter.name,
                    if parameter.mutable { " (mutable)" } else { "" },
                    described.name,
                    if described.mutable { " (mutable)" } else { "" },
                ));
            }
            if !names(&described.ty, &parameter.ty) {
                return Err(format!(
                    "{what}: parameter '{}' is {:?} in the header and {:?} in the record",
                    parameter.name, parameter.ty, described.ty
                ));
            }
        }
        if !names(&declared.return_ty, &function.ret) {
            return Err(format!(
                "{what} returns {:?} in the header and {:?} in the record",
                function.ret, declared.return_ty
            ));
        }
    }
    let exported = |id: &Uuid| header.exports.iter().any(|export| export.id() == id);
    if let Some((id, extra)) = record.exports.iter().find(|(id, _)| !exported(id)) {
        return Err(format!(
            "the record's export '{}' ({id}) is not in the header",
            extra.name
        ));
    }
    Ok(())
}

/// Whether the frozen type `frozen` is the header's `type_ref` frozen, each
/// reference at the version `frozen` pins: a primitive id as that primitive
/// (a `Key` as the string), any other id as a reference to it — as a scalar,
/// an array or an optional, as the header has it. One form per header type,
/// the one the module macros and the derive produce. A fixed-length array or
/// a map has no frozen form, so nothing names it.
fn names(frozen: &FrozenTy, type_ref: &TypeRef) -> bool {
    // A frozen type pinning no reference is a primitive: any version freezes
    // the header's type alike.
    let version = pinned(frozen).cloned().unwrap_or_else(|| {
        Version::from(SemanticVersion {
            major: 0,
            minor: 0,
            patch: 0,
        })
    });
    FrozenTy::from_type_ref(type_ref, &version).as_ref() == Some(frozen)
}

/// The version the record reference in `frozen` pins, if it holds one.
fn pinned(frozen: &FrozenTy) -> Option<&Version> {
    match frozen {
        FrozenTy::FrozenScalar(scalar) => Some(&scalar.reference.version),
        FrozenTy::FrozenArray(array) => Some(&array.reference.version),
        FrozenTy::FrozenOption(option) => pinned(&option.element),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::ty;

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

    /// A record's type names the header's in one form: a primitive id as that
    /// primitive — a `Key` parameter as the string — and any other id as a
    /// reference to it, at whatever version the record pins.
    #[test]
    fn a_record_s_type_names_the_header_s_in_one_form() {
        let version = Version::parse("1.2.0").expect("a version");
        let reference = |id: Uuid| arora_types::record::FrozenReference {
            id,
            version: version.clone(),
        };
        let string = TypeRef::Scalar { id: *ty::STRING_ID };
        assert!(names(&FrozenTy::from(PrimitiveKind::String), &string));
        assert!(!names(
            &FrozenTy::FrozenScalar(arora_types::record::ty::FrozenScalar {
                reference: reference(*ty::STRING_ID)
            }),
            &string
        ));
        assert!(names(
            &option_of(PrimitiveKind::String),
            &TypeRef::Option { id: *ty::STRING_ID }
        ));
        assert!(names(
            &FrozenTy::from(PrimitiveKind::ArrayString),
            &TypeRef::Array { id: *ty::STRING_ID }
        ));

        let record = Uuid::from_u128(0x276);
        let scalar = FrozenTy::FrozenScalar(arora_types::record::ty::FrozenScalar {
            reference: reference(record),
        });
        assert!(names(&scalar, &TypeRef::Scalar { id: record }));
        assert!(!names(&scalar, &TypeRef::Scalar { id: *ty::U8_ID }));
        assert!(names(
            &FrozenTy::FrozenArray(arora_types::record::ty::FrozenArray {
                reference: reference(record)
            }),
            &TypeRef::Array { id: record }
        ));
        assert!(names(
            &FrozenTy::FrozenOption(FrozenOption {
                element: Box::new(scalar.clone())
            }),
            &TypeRef::Option { id: record }
        ));
        assert!(!names(&scalar, &TypeRef::Array { id: record }));
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
