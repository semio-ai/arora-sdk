//! A Rust type that describes itself as an arora [`low::Type`].

use uuid::Uuid;

use crate::ty::{low, TypeRegistry};

/// A Rust type that can produce its own arora [`low::Type`] and the
/// [`TypeRegistry`] of the types it depends on.
///
/// Derive it with `#[derive(AroraType)]` (the `derive` feature): the Rust
/// definition becomes the source of truth for the schema, so a type need not be
/// hand-authored in YAML to take part in a type-directed walk
/// ([`crate::value_serde::write_value`]) or a schema-seeded serde conversion.
///
/// A type is *named* by its [`arora_type_id`](AroraType::arora_type_id) — that
/// same id appears in the [`TypeRef`](crate::module::low::TypeRef) any field
/// referencing it carries. Its full definition
/// ([`arora_type`](AroraType::arora_type)) names nested user-defined types by
/// id; those definitions live in the registry that
/// [`register_types`](AroraType::register_types) fills.
pub trait AroraType {
  /// The id this type is known by: the id its [`arora_type`] carries, and the
  /// id a field of this type is referenced by.
  ///
  /// [`arora_type`]: AroraType::arora_type
  fn arora_type_id() -> Uuid;

  /// This type's own definition. Nested user-defined types are named by id in
  /// the field type references; their definitions are obtained from the
  /// registry filled by [`register_types`](AroraType::register_types).
  fn arora_type() -> low::Type;

  /// Insert this type and every type it transitively depends on into
  /// `registry`. Idempotent, and safe for types reachable from themselves.
  fn register_types(registry: &mut TypeRegistry);

  /// This type's definition together with a registry holding it and all its
  /// dependencies — everything a walk needs to resolve a value of this type.
  fn arora_type_with_registry() -> (low::Type, TypeRegistry) {
    let mut registry = TypeRegistry::new();
    Self::register_types(&mut registry);
    (Self::arora_type(), registry)
  }
}

#[cfg(all(test, feature = "derive"))]
mod tests {
  // Brings both the trait (for `Outer::arora_type()`) and the derive macro
  // (for `#[derive(AroraType)]`) into scope — same name, different namespaces.
  use crate::AroraType;
  use crate::module::low::TypeRef;
  use crate::ty::low::TypeKind;
  use crate::{gen_uuid_from_str, ty};

  #[derive(AroraType)]
  struct Inner {
    a: i32,
    b: f32,
  }

  #[derive(AroraType)]
  struct Outer {
    inner: Inner,
    name: String,
    x: f64,
  }

  #[test]
  fn derive_reproduces_the_structure_type() {
    let g = gen_uuid_from_str;
    let outer = Outer::arora_type();
    assert_eq!(outer.name, "Outer");
    assert_eq!(outer.id, g("Outer"));

    let TypeKind::Structure(structure) = &outer.kind else {
      panic!("expected a structure type");
    };
    // Fields keep declared order: inner, name, x.
    let keys: Vec<_> = structure.fields.keys().copied().collect();
    assert_eq!(keys, vec![g("inner"), g("name"), g("x")]);

    // `inner` references the nested type by its id; `name`/`x` are primitives.
    assert!(matches!(
      &structure.fields[&g("inner")].type_ref,
      TypeRef::Scalar { id } if *id == Inner::arora_type_id()
    ));
    assert!(matches!(
      &structure.fields[&g("name")].type_ref,
      TypeRef::Scalar { id } if *id == *ty::STRING_ID
    ));
    assert!(matches!(
      &structure.fields[&g("x")].type_ref,
      TypeRef::Scalar { id } if *id == *ty::F64_ID
    ));
  }

  #[test]
  fn register_types_collects_dependencies() {
    let (_, registry) = Outer::arora_type_with_registry();
    assert_eq!(registry.len(), 2);
    assert!(registry.contains_key(&Outer::arora_type_id()));
    assert!(registry.contains_key(&Inner::arora_type_id()));
  }

  #[test]
  fn explicit_id_overrides_the_name_hash() {
    #[derive(AroraType)]
    #[arora(id = "11111111-1111-4111-8111-111111111111")]
    struct Pinned {
      #[arora(id = "22222222-2222-4222-8222-222222222222")]
      value: i32,
    }

    let type_id = crate::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let field_id = crate::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    assert_eq!(Pinned::arora_type_id(), type_id);
    let TypeKind::Structure(structure) = &Pinned::arora_type().kind else {
      panic!("expected a structure type");
    };
    assert_eq!(structure.fields.keys().next().copied(), Some(field_id));
  }
}
