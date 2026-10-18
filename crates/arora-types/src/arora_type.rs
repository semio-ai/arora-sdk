//! A Rust type that describes itself as an arora [`low::Type`] and as a type
//! record.

use uuid::Uuid;

use crate::record::scope::Scope;
use crate::record::{TypeDefinitionFrozen, TypeRecords};
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

  /// The scope this type states its record lives in
  /// (`#[arora(scope = …)]`), or `None` when it states none. A type stating
  /// its scope has its record there whatever names it, so modules of
  /// different scopes sharing the type ship one record.
  fn scope() -> Option<Scope> {
    None
  }

  /// This type's record: the structure or enumeration a registry holds under
  /// [`arora_type_id`](AroraType::arora_type_id) at
  /// [`arora_type_version`](AroraType::arora_type_version). Each type it names
  /// is pinned at that type's own version.
  ///
  /// The record lives in the scope the type states
  /// ([`scope`](AroraType::scope)); a type stating none lives in
  /// `default_scope`, the scope of what names it: the module whose function
  /// names it, or the type whose field names it. With neither, it lives at
  /// the root.
  ///
  /// `None` for a type with no record of its own — one that travels as a
  /// well-known type, as a [`Key`](crate::data::Key) travels as a string —
  /// and for one whose shape a record cannot express: a fixed-length array.
  fn record(_default_scope: Option<Scope>) -> Option<TypeDefinitionFrozen> {
    None
  }

  /// Insert this type's [`record`](AroraType::record), and the record of
  /// every type it names, into `records`, each under the reference that pins
  /// it, with the scopes they live in. This type, when it states no scope,
  /// lives in `default_scope`; a type it names that states none lives where
  /// this one does. A type reached first through one path keeps the scope
  /// that path gives it. Idempotent, and safe for a type reachable from
  /// itself. A type without a record inserts nothing, so a record naming one
  /// names a record `records` does not hold: a registry's
  /// `missing_dependencies` reports it.
  fn register_records(_default_scope: Option<Scope>, _records: &mut TypeRecords) {}
}

#[cfg(all(test, feature = "derive"))]
mod tests {
  // Brings both the trait (for `Outer::arora_type()`) and the derive macro
  // (for `#[derive(AroraType)]`) into scope — same name, different namespaces.
  use crate::module::low::TypeRef;
  use crate::ty::low::TypeKind;
  use crate::value::Value;
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
  fn a_boxed_self_reference_and_a_unit_field_round_trip() {
    #[derive(AroraType, Debug, Clone, PartialEq)]
    #[arora(id = "cccccccc-cccc-4ccc-8ccc-000000000001")]
    struct Chain {
      #[arora(id = "cccccccc-cccc-4ccc-8ccc-000000000011")]
      head: i32,
      #[arora(id = "cccccccc-cccc-4ccc-8ccc-000000000012")]
      tail: Option<Box<Chain>>,
      #[arora(id = "cccccccc-cccc-4ccc-8ccc-000000000013")]
      marker: (),
    }
    #[derive(AroraType, Debug, Clone, PartialEq)]
    #[arora(id = "cccccccc-cccc-4ccc-8ccc-000000000002")]
    struct Holder {
      #[arora(id = "cccccccc-cccc-4ccc-8ccc-000000000021")]
      chain: Box<Chain>,
    }

    let TypeKind::Structure(structure) = Holder::arora_type().kind else {
      panic!("expected a structure type");
    };
    // A box is referenced as the type it holds.
    assert!(matches!(
      &structure.fields.values().next().unwrap().type_ref,
      TypeRef::Scalar { id } if *id == Chain::arora_type_id()
    ));
    let TypeKind::Structure(chain) = Chain::arora_type().kind else {
      panic!("expected a structure type");
    };
    assert!(matches!(
      &chain.fields[&crate::Uuid::parse_str("cccccccc-cccc-4ccc-8ccc-000000000013").unwrap()].type_ref,
      TypeRef::Scalar { id } if *id == *ty::UNIT_ID
    ));

    let holder = Holder {
      chain: Box::new(Chain {
        head: 1,
        tail: Some(Box::new(Chain {
          head: 2,
          tail: None,
          marker: (),
        })),
        marker: (),
      }),
    };
    let value = Value::from(holder.clone());
    let Value::Structure(outer) = &value else {
      panic!("a structure value");
    };
    // The box is transparent: the field holds the chain's own structure.
    assert!(
      matches!(&*outer.fields[0].value, Value::Structure(s) if s.id == Chain::arora_type_id())
    );
    assert_eq!(Holder::try_from(value).unwrap(), holder);
  }

  #[test]
  fn an_optional_field_reads_absent_bare_or_framed() {
    #[derive(AroraType, Debug, Clone, PartialEq)]
    #[arora(id = "dddddddd-dddd-4ddd-8ddd-000000000001")]
    struct Maybe {
      #[arora(id = "dddddddd-dddd-4ddd-8ddd-000000000011")]
      rate: Option<f32>,
    }
    let id = Maybe::arora_type_id();
    let field_id = crate::Uuid::parse_str("dddddddd-dddd-4ddd-8ddd-000000000011").unwrap();
    let with = |fields| Value::Structure(crate::value::Structure { id, fields });
    let field = |value| {
      vec![crate::value::StructureField {
        id: field_id,
        value: Box::new(value),
      }]
    };
    assert_eq!(Maybe::try_from(with(vec![])).unwrap(), Maybe { rate: None });
    assert_eq!(
      Maybe::try_from(with(field(Value::F32(2.0)))).unwrap(),
      Maybe { rate: Some(2.0) }
    );
    assert_eq!(
      Maybe::try_from(with(field(Value::Option(Some(Box::new(Value::F32(3.0))))))).unwrap(),
      Maybe { rate: Some(3.0) }
    );
    assert_eq!(
      Maybe::try_from(with(field(Value::Option(None)))).unwrap(),
      Maybe { rate: None }
    );
    assert!(Maybe::try_from(with(field(Value::String("fast".into())))).is_err());
  }

  #[test]
  fn an_empty_array_of_an_enumeration_is_an_enumeration_array() {
    #[derive(AroraType, Debug, Clone, Copy, PartialEq)]
    #[arora(id = "eeeeeeee-eeee-4eee-8eee-000000000001")]
    enum Mode {
      #[arora(id = "eeeeeeee-eeee-4eee-8eee-000000000011")]
      On,
    }
    #[derive(AroraType, Debug, Clone, PartialEq)]
    #[arora(id = "eeeeeeee-eeee-4eee-8eee-000000000002")]
    struct Modes {
      #[arora(id = "eeeeeeee-eeee-4eee-8eee-000000000021")]
      modes: Vec<Mode>,
    }
    let Value::Structure(structure) = Value::from(Modes { modes: vec![] }) else {
      panic!("a structure value");
    };
    assert_eq!(
      *structure.fields[0].value,
      Value::ArrayEnumeration {
        id: Mode::arora_type_id(),
        elements: vec![]
      }
    );
  }

  #[test]
  fn a_field_and_a_variant_take_their_arora_name() {
    #[derive(AroraType)]
    #[arora(id = "ffffffff-ffff-4fff-8fff-000000000001")]
    struct Renamed {
      #[arora(id = "ffffffff-ffff-4fff-8fff-000000000011", name = "callableId")]
      callable_id: u64,
    }
    #[derive(AroraType)]
    #[arora(id = "ffffffff-ffff-4fff-8fff-000000000002")]
    enum Light {
      #[arora(id = "ffffffff-ffff-4fff-8fff-000000000021", name = "ON")]
      On,
    }
    let TypeKind::Structure(structure) = Renamed::arora_type().kind else {
      panic!("expected a structure type");
    };
    assert_eq!(structure.fields.values().next().unwrap().name, "callableId");
    let TypeKind::Enumeration(enumeration) = Light::arora_type().kind else {
      panic!("expected an enumeration type");
    };
    assert_eq!(enumeration.values.values().next().unwrap().name, "ON");
  }

  // ---- the records the derive emits -----------------------------------------

  /// A struct's record is a structure in the scope given, its fields in
  /// declared order, each typed as a record names it: a primitive by kind, a
  /// nested type by a reference pinned at that type's version, a well-known
  /// type (a uuid, a dynamic value) by a reference at 1.0.0.
  #[test]
  fn a_struct_s_record_pins_each_type_it_names_at_that_type_s_version() {
    use crate::record::ty::{FrozenArray, FrozenOption, FrozenScalar, FrozenTy, PrimitiveKind};
    use crate::record::{FrozenReference, TypeDefinitionFrozen, Version};

    #[derive(AroraType)]
    #[arora(id = "12121212-1212-4121-8121-000000000001", version = "1.2.0")]
    struct Leaf {
      #[arora(id = "12121212-1212-4121-8121-000000000011")]
      x: f32,
    }
    #[derive(AroraType)]
    #[arora(id = "12121212-1212-4121-8121-000000000002", version = "2.0.1")]
    struct Shape {
      #[arora(id = "12121212-1212-4121-8121-000000000021")]
      count: u32,
      #[arora(id = "12121212-1212-4121-8121-000000000022")]
      leaf: Leaf,
      #[arora(id = "12121212-1212-4121-8121-000000000023")]
      leaves: Vec<Leaf>,
      #[arora(id = "12121212-1212-4121-8121-000000000024")]
      maybe: Option<Box<Leaf>>,
      #[arora(id = "12121212-1212-4121-8121-000000000025")]
      samples: Vec<f64>,
      #[arora(id = "12121212-1212-4121-8121-000000000026")]
      limit: Option<u64>,
      #[arora(id = "12121212-1212-4121-8121-000000000027")]
      source: crate::Uuid,
      #[arora(id = "12121212-1212-4121-8121-000000000028", keyvalue)]
      payload: Value,
      #[arora(id = "12121212-1212-4121-8121-000000000029")]
      key: crate::data::Key,
    }

    let scope = crate::record::scope::Scope {
      id: crate::Uuid::from_u128(0xf01d),
      handle: "shapes",
    };
    let Some(TypeDefinitionFrozen::Structure(record)) = Shape::record(Some(scope)) else {
      panic!("a struct's record is a structure");
    };
    assert_eq!(record.parent, scope.id);
    assert_eq!(record.name, "Shape");
    let names: Vec<&str> = record.fields.values().map(|f| f.name.as_str()).collect();
    assert_eq!(
      names,
      ["count", "leaf", "leaves", "maybe", "samples", "limit", "source", "payload", "key"]
    );
    let leaf = FrozenReference {
      id: Leaf::arora_type_id(),
      version: Version::parse("1.2.0").unwrap(),
    };
    let well_known = |id: crate::Uuid| FrozenReference {
      id,
      version: Version::parse("1.0.0").unwrap(),
    };
    let types: Vec<FrozenTy> = record.fields.values().map(|f| f.ty.clone()).collect();
    assert_eq!(
      types,
      vec![
        FrozenTy::from(PrimitiveKind::U32),
        FrozenTy::FrozenScalar(FrozenScalar {
          reference: leaf.clone()
        }),
        FrozenTy::FrozenArray(FrozenArray {
          reference: leaf.clone()
        }),
        FrozenTy::FrozenOption(FrozenOption {
          element: Box::new(FrozenTy::FrozenScalar(FrozenScalar { reference: leaf })),
        }),
        FrozenTy::from(PrimitiveKind::ArrayF64),
        FrozenTy::FrozenOption(FrozenOption {
          element: Box::new(FrozenTy::from(PrimitiveKind::U64)),
        }),
        FrozenTy::FrozenScalar(FrozenScalar {
          reference: well_known(*ty::UUID_ID)
        }),
        FrozenTy::FrozenScalar(FrozenScalar {
          reference: well_known(*ty::KEY_VALUE_ID)
        }),
        // A key travels as a string.
        FrozenTy::from(PrimitiveKind::String),
      ]
    );
  }

  /// An enum's record is an enumeration whose variants carry unit, as the
  /// value plane's enumerations do.
  #[test]
  fn an_enum_s_record_is_an_enumeration_of_unit_variants() {
    use crate::record::ty::{FrozenTy, PrimitiveKind};
    use crate::record::TypeDefinitionFrozen;

    #[derive(AroraType)]
    #[arora(id = "13131313-1313-4131-8131-000000000001")]
    enum Mode {
      #[arora(id = "13131313-1313-4131-8131-000000000011")]
      Idle,
      #[arora(id = "13131313-1313-4131-8131-000000000012", name = "RUN")]
      Run,
    }
    let Some(TypeDefinitionFrozen::Enumeration(record)) = Mode::record(None) else {
      panic!("an enum's record is an enumeration");
    };
    assert_eq!(record.name, "Mode");
    // Without a scope, it lives at the root.
    assert_eq!(record.parent, crate::record::folder::ROOT_ID);
    let variants: Vec<(String, String, FrozenTy)> = record
      .variants
      .iter()
      .map(|(id, variant)| (id.to_string(), variant.name.clone(), variant.ty.clone()))
      .collect();
    let unit = FrozenTy::from(PrimitiveKind::Unit);
    assert_eq!(
      variants,
      vec![
        (
          "13131313-1313-4131-8131-000000000011".to_string(),
          "Idle".to_string(),
          unit.clone()
        ),
        (
          "13131313-1313-4131-8131-000000000012".to_string(),
          "RUN".to_string(),
          unit
        ),
      ]
    );
  }

  /// `register_records` holds a type's record and those of every type it
  /// names, transitively, each once — a type reachable from itself included —
  /// under the references that pin them. Types stating no scope all live in
  /// the scope given, which is the one scope the records name.
  #[test]
  fn register_records_collects_every_record_a_type_reaches_once() {
    use crate::record::{FrozenReference, TypeDefinitionFrozen, TypeRecords};

    #[derive(AroraType)]
    #[arora(id = "14141414-1414-4141-8141-000000000001")]
    enum Kind {
      #[arora(id = "14141414-1414-4141-8141-000000000011")]
      Leaf,
    }
    #[derive(AroraType)]
    #[arora(id = "14141414-1414-4141-8141-000000000002", version = "3.0.0")]
    struct Node {
      #[arora(id = "14141414-1414-4141-8141-000000000021")]
      kind: Kind,
      #[arora(id = "14141414-1414-4141-8141-000000000022")]
      children: Vec<Node>,
      #[arora(id = "14141414-1414-4141-8141-000000000023")]
      key: crate::data::Key,
    }
    #[derive(AroraType)]
    #[arora(id = "14141414-1414-4141-8141-000000000003")]
    struct Tree {
      #[arora(id = "14141414-1414-4141-8141-000000000031")]
      root: Option<Box<Node>>,
    }

    let scope = crate::record::scope::Scope {
      id: crate::Uuid::from_u128(0xf01d),
      handle: "forest",
    };
    let mut records = TypeRecords::default();
    Tree::register_records(Some(scope), &mut records);
    Tree::register_records(Some(scope), &mut records);
    let reference = |id: crate::Uuid, version: &str| FrozenReference {
      id,
      version: crate::record::Version::parse(version).unwrap(),
    };
    assert_eq!(
      records
        .types
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>(),
      std::collections::BTreeSet::from([
        reference(Tree::arora_type_id(), "1.0.0"),
        reference(Node::arora_type_id(), "3.0.0"),
        reference(Kind::arora_type_id(), "1.0.0"),
      ])
    );
    for record in records.types.values() {
      let parent = match record {
        TypeDefinitionFrozen::Structure(s) => s.parent,
        TypeDefinitionFrozen::Enumeration(e) => e.parent,
        TypeDefinitionFrozen::Primitive(_) => panic!("a primitive has no record"),
      };
      assert_eq!(parent, scope.id);
    }
    assert_eq!(records.scopes, std::collections::BTreeSet::from([scope]));
  }

  /// A type stating its scope (`#[arora(scope = …)]`) has its record there
  /// whatever names it, and a type it names that states none lives there
  /// too; a type stating none that the caller names directly lives in the
  /// scope the caller gives. The records name both scopes.
  #[test]
  fn a_type_stating_its_scope_keeps_it_whatever_names_it() {
    use crate::record::scope::Scope;
    use crate::record::{FrozenReference, TypeDefinitionFrozen, TypeRecords};
    use crate::Uuid;

    const SHAPES: Scope = Scope {
      id: Uuid::from_u128(0x5a9e),
      handle: "shapes",
    };

    #[derive(AroraType)]
    #[arora(id = "16161616-1616-4161-8161-000000000001")]
    enum Winding {
      #[arora(id = "16161616-1616-4161-8161-000000000011")]
      Clockwise,
    }
    #[derive(AroraType)]
    #[arora(id = "16161616-1616-4161-8161-000000000002", scope = SHAPES)]
    struct Polygon {
      #[arora(id = "16161616-1616-4161-8161-000000000021")]
      winding: Winding,
    }
    #[derive(AroraType)]
    #[arora(id = "16161616-1616-4161-8161-000000000003")]
    struct Drawing {
      #[arora(id = "16161616-1616-4161-8161-000000000031")]
      outline: Polygon,
    }

    assert_eq!(Polygon::scope(), Some(SHAPES));
    assert_eq!(Drawing::scope(), None);

    let parent_of = |records: &TypeRecords, id: Uuid| {
      let reference = FrozenReference {
        id,
        version: crate::record::Version::parse("1.0.0").unwrap(),
      };
      match records.types.get(&reference) {
        Some(TypeDefinitionFrozen::Structure(s)) => s.parent,
        Some(TypeDefinitionFrozen::Enumeration(e)) => e.parent,
        other => panic!("{id} has no record: {other:?}"),
      }
    };
    let module_scope = Scope {
      id: Uuid::from_u128(0x6d0d),
      handle: "studio",
    };
    let mut records = TypeRecords::default();
    Drawing::register_records(Some(module_scope), &mut records);
    assert_eq!(
      parent_of(&records, Drawing::arora_type_id()),
      module_scope.id
    );
    assert_eq!(parent_of(&records, Polygon::arora_type_id()), SHAPES.id);
    assert_eq!(parent_of(&records, Winding::arora_type_id()), SHAPES.id);
    assert_eq!(
      records.scopes,
      std::collections::BTreeSet::from([module_scope, SHAPES])
    );

    // Named from another scope, or from none, the shared type and the type
    // it names are the same records.
    for elsewhere_scope in [
      Some(Scope {
        id: Uuid::from_u128(0xe15e),
        handle: "elsewhere",
      }),
      None,
    ] {
      let mut elsewhere = TypeRecords::default();
      Polygon::register_records(elsewhere_scope, &mut elsewhere);
      assert_eq!(elsewhere.types.len(), 2);
      for (reference, record) in &elsewhere.types {
        assert_eq!(records.types.get(reference), Some(record));
      }
      assert_eq!(elsewhere.scopes, std::collections::BTreeSet::from([SHAPES]));
    }
  }

  /// A fixed-length array has no record form, so neither has a type holding
  /// one; it registers no record.
  #[test]
  fn a_type_with_a_fixed_length_array_has_no_record() {
    #[derive(AroraType)]
    #[arora(id = "15151515-1515-4151-8151-000000000001")]
    struct Pose {
      #[arora(id = "15151515-1515-4151-8151-000000000011")]
      covariance: [f64; 4],
    }
    assert!(Pose::record(None).is_none());
    let mut records = crate::record::TypeRecords::default();
    Pose::register_records(None, &mut records);
    assert_eq!(records, crate::record::TypeRecords::default());
  }
}
