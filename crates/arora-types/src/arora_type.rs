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

  /// The record version this type is pinned at wherever a frozen form names
  /// it — a described function's signature, a module record's dependencies.
  /// `1.0.0` unless the derive is told otherwise with `#[arora(version = "…")]`.
  fn arora_type_version() -> crate::record::Version {
    crate::record::Version::from(crate::SemanticVersion {
      major: 1,
      minor: 0,
      patch: 0,
    })
  }

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
  use crate::module::low::TypeRef;
  use crate::ty::low::TypeKind;
  use crate::AroraType;
  use crate::{gen_uuid_from_str, ty};

  // Name-mode (opts into name-hashing) so these fixtures need no explicit ids
  // and their ids stay the `gen_uuid_from_str` values the assertions expect.
  #[derive(AroraType)]
  #[arora(name = "Inner")]
  struct Inner {
    a: i32,
    b: f32,
  }

  #[derive(AroraType)]
  #[arora(name = "Outer")]
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

  #[test]
  fn derive_reproduces_an_enumeration_with_pinned_variant_ids() {
    // A unit-variant enum with pinned type and variant ids — the value-plane
    // `Status` shape: the derive must reproduce the enumeration id and each
    // variant id exactly, so the wire form is stable.
    #[derive(AroraType)]
    #[arora(id = "325a5767-e344-4532-860e-0749bcf2e428")]
    enum Status {
      #[arora(id = "766e9e9a-446d-4e46-83e6-14b7ca101169")]
      Success,
      #[arora(id = "2468f46c-bb60-425c-9a4d-9ad326ccc7e2")]
      Failure,
      #[arora(id = "acd79ec6-0c44-401a-82f8-5da5422d3eec")]
      Running,
    }

    let parse = |s| crate::Uuid::parse_str(s).unwrap();
    assert_eq!(
      Status::arora_type_id(),
      parse("325a5767-e344-4532-860e-0749bcf2e428")
    );
    let ty = Status::arora_type();
    assert_eq!(ty.name, "Status");
    let TypeKind::Enumeration(enumeration) = &ty.kind else {
      panic!("expected an enumeration type");
    };
    // Variants keep declared order, each keyed by its pinned id, each a unit
    // payload.
    let keys: Vec<_> = enumeration.values.keys().copied().collect();
    assert_eq!(
      keys,
      vec![
        parse("766e9e9a-446d-4e46-83e6-14b7ca101169"),
        parse("2468f46c-bb60-425c-9a4d-9ad326ccc7e2"),
        parse("acd79ec6-0c44-401a-82f8-5da5422d3eec"),
      ]
    );
    assert_eq!(
      enumeration.values[&parse("766e9e9a-446d-4e46-83e6-14b7ca101169")].name,
      "Success"
    );
    assert!(matches!(
      &enumeration.values[&parse("acd79ec6-0c44-401a-82f8-5da5422d3eec")].type_ref,
      TypeRef::Scalar { id } if *id == *ty::UNIT_ID
    ));
  }

  #[test]
  fn uuid_and_option_fields() {
    #[derive(AroraType)]
    #[arora(id = "33333333-3333-4333-8333-333333333333")]
    struct Ref {
      #[arora(id = "44444444-4444-4444-8444-444444444444")]
      id: crate::Uuid,
      #[arora(id = "55555555-5555-4555-8555-555555555555")]
      maybe: Option<crate::Uuid>,
    }

    let TypeKind::Structure(structure) = &Ref::arora_type().kind else {
      panic!("expected a structure type");
    };
    let g = |s: &str| crate::Uuid::parse_str(s).unwrap();
    // A `Uuid` field is a scalar of the well-known UUID primitive.
    assert!(matches!(
      structure.fields[&g("44444444-4444-4444-8444-444444444444")].type_ref,
      TypeRef::Scalar { id } if id == *ty::UUID_ID
    ));
    // An `Option<Uuid>` field is the new optional-of-uuid type ref.
    assert!(matches!(
      structure.fields[&g("55555555-5555-4555-8555-555555555555")].type_ref,
      TypeRef::Option { id } if id == *ty::UUID_ID
    ));
  }

  // ---- the value-plane conversions the derive emits ----------------------

  #[test]
  fn a_struct_round_trips_as_a_structure_under_its_ids() {
    use crate::value::{Structure, StructureField, Value};
    #[derive(Debug, Clone, PartialEq, AroraType)]
    #[arora(id = "66666666-6666-4666-8666-666666666666")]
    struct Reading {
      #[arora(id = "66666666-6666-4666-8666-000000000001")]
      name: String,
      #[arora(id = "66666666-6666-4666-8666-000000000002")]
      value: f32,
      #[arora(id = "66666666-6666-4666-8666-000000000003")]
      source: Option<crate::Uuid>,
      #[arora(id = "66666666-6666-4666-8666-000000000004")]
      samples: Vec<u8>,
      #[arora(id = "66666666-6666-4666-8666-000000000005")]
      window: [f64; 2],
    }
    let g = |s: &str| crate::Uuid::parse_str(s).unwrap();
    let reading = Reading {
      name: "temp".into(),
      value: 21.5,
      source: None,
      samples: vec![1, 2, 3],
      window: [0.5, 1.5],
    };
    let value = Value::from(reading.clone());
    // Fields in declared order, each under its id; arrays in their typed form.
    assert_eq!(
      value,
      Value::Structure(Structure {
        id: g("66666666-6666-4666-8666-666666666666"),
        fields: vec![
          StructureField {
            id: g("66666666-6666-4666-8666-000000000001"),
            value: Box::new(Value::String("temp".into()))
          },
          StructureField {
            id: g("66666666-6666-4666-8666-000000000002"),
            value: Box::new(Value::F32(21.5))
          },
          StructureField {
            id: g("66666666-6666-4666-8666-000000000003"),
            value: Box::new(Value::Option(None))
          },
          StructureField {
            id: g("66666666-6666-4666-8666-000000000004"),
            value: Box::new(Value::ArrayU8(vec![1, 2, 3]))
          },
          StructureField {
            id: g("66666666-6666-4666-8666-000000000005"),
            value: Box::new(Value::ArrayF64(vec![0.5, 1.5]))
          },
        ],
      })
    );
    assert_eq!(Reading::try_from(value).unwrap(), reading);
    // A structure of another id is refused; a fixed array of the wrong length too.
    let wrong = Value::Structure(Structure {
      id: g("77777777-7777-4777-8777-777777777777"),
      fields: vec![],
    });
    assert!(Reading::try_from(wrong)
      .unwrap_err()
      .message
      .contains("expected a `Reading` structure"));
  }

  #[test]
  fn nested_types_and_arrays_of_them_round_trip() {
    use crate::value::Value;
    #[derive(Debug, Clone, PartialEq, AroraType)]
    #[arora(id = "88888888-8888-4888-8888-000000000001")]
    struct Point {
      #[arora(id = "88888888-8888-4888-8888-000000000011")]
      x: f32,
    }
    #[derive(Debug, Clone, PartialEq, AroraType)]
    #[arora(id = "88888888-8888-4888-8888-000000000002")]
    struct Path {
      #[arora(id = "88888888-8888-4888-8888-000000000021")]
      start: Point,
      #[arora(id = "88888888-8888-4888-8888-000000000022")]
      points: Vec<Point>,
    }
    let path = Path {
      start: Point { x: 0.0 },
      points: vec![Point { x: 1.0 }, Point { x: 2.0 }],
    };
    let value = Value::from(path.clone());
    let Value::Structure(s) = &value else {
      panic!("a structure")
    };
    assert!(matches!(&*s.fields[0].value, Value::Structure(p) if p.id == Point::arora_type_id()));
    assert!(
      matches!(&*s.fields[1].value, Value::ArrayStructure { id, elements } if *id == Point::arora_type_id() && elements.len() == 2)
    );
    assert_eq!(Path::try_from(value).unwrap(), path);
    // An empty array of a structure type keeps the structure form.
    let empty = Value::from(Path {
      start: Point { x: 0.0 },
      points: vec![],
    });
    let Value::Structure(s) = &empty else {
      panic!("a structure")
    };
    assert!(
      matches!(&*s.fields[1].value, Value::ArrayStructure { elements, .. } if elements.is_empty())
    );
  }

  #[test]
  fn a_unit_enum_round_trips_as_the_enumeration_the_value_plane_speaks() {
    use crate::value::{Enumeration, Value};
    #[derive(Debug, Clone, Copy, PartialEq, AroraType)]
    #[arora(id = "325a5767-e344-4532-860e-0749bcf2e428")]
    enum Status {
      #[arora(id = "766e9e9a-446d-4e46-83e6-14b7ca101169")]
      Success,
      #[arora(id = "2468f46c-bb60-425c-9a4d-9ad326ccc7e2")]
      Failure,
    }
    let parse = |s| crate::Uuid::parse_str(s).unwrap();
    assert_eq!(
      Value::from(Status::Failure),
      Value::Enumeration(Enumeration {
        id: parse("325a5767-e344-4532-860e-0749bcf2e428"),
        variant_id: parse("2468f46c-bb60-425c-9a4d-9ad326ccc7e2"),
        value: Box::new(Value::Unit),
      })
    );
    assert_eq!(
      Status::try_from(Value::from(Status::Success)).unwrap(),
      Status::Success
    );
    let foreign = Value::Enumeration(Enumeration {
      id: parse("325a5767-e344-4532-860e-0749bcf2e428"),
      variant_id: parse("00000000-0000-4000-8000-000000000000"),
      value: Box::new(Value::Unit),
    });
    assert!(Status::try_from(foreign)
      .unwrap_err()
      .message
      .contains("unknown variant"));
  }

  #[test]
  fn a_keyvalue_field_passes_a_value_through_and_serializes_anything_else() {
    use crate::value::Value;
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Extra {
      note: String,
    }
    #[derive(Debug, Clone, PartialEq, AroraType)]
    #[arora(id = "99999999-9999-4999-8999-000000000001")]
    struct Envelope {
      #[arora(id = "99999999-9999-4999-8999-000000000011", keyvalue)]
      payload: Value,
      #[arora(id = "99999999-9999-4999-8999-000000000012", keyvalue)]
      extra: Extra,
    }
    let envelope = Envelope {
      payload: Value::F32(0.5),
      extra: Extra { note: "n".into() },
    };
    let value = Value::from(envelope.clone());
    let Value::Structure(s) = &value else {
      panic!("a structure")
    };
    assert_eq!(*s.fields[0].value, Value::F32(0.5));
    assert!(matches!(&*s.fields[1].value, Value::KeyValue(_)));
    assert_eq!(Envelope::try_from(value).unwrap(), envelope);
  }

  #[test]
  fn a_field_named_fields_or_id_does_not_confuse_the_generated_code() {
    use crate::value::Value;
    #[derive(Debug, Clone, PartialEq, AroraType)]
    #[arora(id = "aaaaaaaa-aaaa-4aaa-8aaa-000000000001")]
    struct Cloud {
      #[arora(id = "aaaaaaaa-aaaa-4aaa-8aaa-000000000011")]
      id: u32,
      #[arora(id = "aaaaaaaa-aaaa-4aaa-8aaa-000000000012")]
      fields: Vec<String>,
      #[arora(id = "aaaaaaaa-aaaa-4aaa-8aaa-000000000013")]
      value: bool,
    }
    let cloud = Cloud {
      id: 7,
      fields: vec!["x".into(), "y".into()],
      value: true,
    };
    assert_eq!(Cloud::try_from(Value::from(cloud.clone())).unwrap(), cloud);
  }

  #[test]
  fn the_version_is_the_types_and_defaults_to_one() {
    #[derive(AroraType)]
    #[arora(id = "bbbbbbbb-bbbb-4bbb-8bbb-000000000001", version = "1.1.0")]
    struct Pinned {
      #[arora(id = "bbbbbbbb-bbbb-4bbb-8bbb-000000000011")]
      x: u8,
    }
    #[derive(AroraType)]
    #[arora(id = "bbbbbbbb-bbbb-4bbb-8bbb-000000000002")]
    struct Unpinned {
      #[arora(id = "bbbbbbbb-bbbb-4bbb-8bbb-000000000021")]
      x: u8,
    }
    assert_eq!(Pinned::arora_type_version().to_string(), "1.1.0");
    assert_eq!(Unpinned::arora_type_version().to_string(), "1.0.0");
  }

  #[test]
  fn an_id_may_be_spelled_in_emoji() {
    // 🎯🚤🧪💲🌼🏪🔘😊🉑🍉⚪🔕♍ is e1b4bda7-1c7b-4322-b9a0-552201b8a011 (arora-id's vector).
    #[derive(AroraType)]
    #[arora(id = "🎯🚤🧪💲🌼🏪🔘😊🉑🍉⚪🔕♍")]
    struct Spelled {
      #[arora(id = "cccccccc-cccc-4ccc-8ccc-000000000011")]
      x: u8,
    }
    assert_eq!(
      Spelled::arora_type_id(),
      crate::Uuid::parse_str("e1b4bda7-1c7b-4322-b9a0-552201b8a011").unwrap()
    );
    assert_eq!(
      crate::id::encode(&Spelled::arora_type_id()),
      "🎯🚤🧪💲🌼🏪🔘😊🉑🍉⚪🔕♍"
    );
  }
}
