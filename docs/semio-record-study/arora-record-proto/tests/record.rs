use std::collections::HashMap;

use arora_record_proto::examples::{
  FrozenTy, InMemoryRegistry, ModuleHeader, Parameter, PrimitiveKind, ResolveError, Structure,
  StructureField, Unfreeze, UnfrozenTy,
};
use arora_record_proto::{Compat, Freeze, Version, VersionReq, Versioned};
use uuid::Uuid;

fn v(s: &str) -> Version {
  Version::parse(s).unwrap()
}

// --- 1. Version tagging + compatibility checks --------------------------------

#[test]
fn version_compat_rules() {
  assert_eq!(Structure::compatibility(&v("1.2.0"), &v("1.2.0")), Compat::Identical);
  // minor bump, same major => backward compatible
  assert_eq!(
    Structure::compatibility(&v("1.2.0"), &v("1.5.0")),
    Compat::BackwardCompatible
  );
  // major bump => incompatible
  assert_eq!(
    Structure::compatibility(&v("1.2.0"), &v("2.0.0")),
    Compat::Incompatible
  );
  // downgrade => incompatible
  assert_eq!(
    Structure::compatibility(&v("1.5.0"), &v("1.2.0")),
    Compat::Incompatible
  );
}

#[test]
fn versioned_reference_tagging() {
  let id = Uuid::new_v4();
  let s = Structure {
    id,
    version: v("3.1.4"),
    name: "Pose".into(),
    fields: HashMap::new(),
  };
  assert_eq!(s.frozen_reference().version, v("3.1.4"));
  assert_eq!(s.frozen_reference().id, id);

  let unfrozen = s.unfrozen_reference(VersionReq::parse(">=3.0.0").unwrap());
  assert_eq!(unfrozen.id, id);
  assert!(unfrozen.version_req.matches(&v("3.2.0")));
  assert!(!unfrozen.version_req.matches(&v("2.9.0")));
}

// --- 2. Resolver picks the newest matching version ----------------------------

#[test]
fn resolver_picks_newest_matching() {
  let id = Uuid::new_v4();
  let mut reg = InMemoryRegistry::new();
  reg.publish(id, v("1.0.0"));
  reg.publish(id, v("1.4.0"));
  reg.publish(id, v("2.0.0"));

  let any = UnfrozenTy::Scalar(arora_record_proto::UnfrozenReference {
    id,
    version_req: VersionReq::any(),
  });
  let frozen = any.freeze(&reg).unwrap();
  match frozen {
    FrozenTy::Scalar(r) => assert_eq!(r.version, v("2.0.0")),
    other => panic!("expected scalar, got {:?}", other),
  }

  let capped = UnfrozenTy::Scalar(arora_record_proto::UnfrozenReference {
    id,
    version_req: VersionReq::parse("<2.0.0").unwrap(),
  });
  match capped.freeze(&reg).unwrap() {
    FrozenTy::Scalar(r) => assert_eq!(r.version, v("1.4.0")),
    other => panic!("expected scalar, got {:?}", other),
  }
}

#[test]
fn resolver_errors_surface() {
  let mut reg = InMemoryRegistry::new();
  let id = Uuid::new_v4();
  reg.publish(id, v("1.0.0"));

  let missing = arora_record_proto::UnfrozenReference {
    id: Uuid::new_v4(),
    version_req: VersionReq::any(),
  };
  assert!(matches!(reg_resolve(&reg, &missing), ResolveError::NoSuchRecord(_)));

  let bad_req = arora_record_proto::UnfrozenReference {
    id,
    version_req: VersionReq::parse(">=2.0.0").unwrap(),
  };
  assert!(matches!(reg_resolve(&reg, &bad_req), ResolveError::NoSuchVersion(_, _)));
}

fn reg_resolve(
  reg: &InMemoryRegistry,
  r: &arora_record_proto::UnfrozenReference,
) -> ResolveError {
  use arora_record_proto::Resolver;
  reg.resolve(r).unwrap_err()
}

// --- 3. Freeze/unfreeze round-trip on a leaf reference ------------------------

#[test]
fn freeze_unfreeze_roundtrip_pins_exact_version() {
  let id = Uuid::new_v4();
  let mut reg = InMemoryRegistry::new();
  reg.publish(id, v("1.0.0"));
  reg.publish(id, v("1.7.2"));

  let original = UnfrozenTy::Array(arora_record_proto::UnfrozenReference {
    id,
    version_req: VersionReq::parse("^1.0").unwrap(),
  });

  let frozen = original.freeze(&reg).unwrap();
  // unfreeze widens back to an exact requirement on the pinned version.
  let widened = frozen.unfreeze();

  // Re-freezing the widened ref must yield the SAME pin: round-trip stable.
  let refrozen = widened.freeze(&reg).unwrap();
  assert_eq!(frozen, refrozen);

  match widened {
    UnfrozenTy::Array(r) => assert!(r.version_req.matches(&v("1.7.2")) && !r.version_req.matches(&v("1.0.0"))),
    other => panic!("expected array, got {:?}", other),
  }
}

// --- 4. The trait applied to >= 2 distinct composite types --------------------

#[test]
fn freeze_structure_record() {
  let dep_id = Uuid::new_v4();
  let mut reg = InMemoryRegistry::new();
  reg.publish(dep_id, v("0.9.0"));
  reg.publish(dep_id, v("1.2.0"));

  let mut fields = HashMap::new();
  fields.insert(
    Uuid::new_v4(),
    StructureField {
      name: "count".into(),
      ty: UnfrozenTy::Primitive(PrimitiveKind::I32),
    },
  );
  fields.insert(
    Uuid::new_v4(),
    StructureField {
      name: "child".into(),
      ty: UnfrozenTy::Scalar(arora_record_proto::UnfrozenReference {
        id: dep_id,
        version_req: VersionReq::parse("^1.0").unwrap(),
      }),
    },
  );

  let s = Structure {
    id: Uuid::new_v4(),
    version: v("1.0.0"),
    name: "Robot".into(),
    fields,
  };

  let frozen = s.freeze(&reg).unwrap();
  let child = frozen.fields.values().find(|f| f.name == "child").unwrap();
  match &child.ty {
    FrozenTy::Scalar(r) => assert_eq!(r.version, v("1.2.0")),
    other => panic!("expected scalar, got {:?}", other),
  }
}

#[test]
fn freeze_module_header_record() {
  let ty_id = Uuid::new_v4();
  let dep_id = Uuid::new_v4();
  let mut reg = InMemoryRegistry::new();
  reg.publish(ty_id, v("2.3.0"));
  reg.publish(dep_id, v("0.1.0"));
  reg.publish(dep_id, v("0.2.0"));

  let header = ModuleHeader {
    id: Uuid::new_v4(),
    version: v("1.0.0"),
    name: "polly".into(),
    parameters: vec![Parameter {
      name: "input".into(),
      ty: UnfrozenTy::Scalar(arora_record_proto::UnfrozenReference {
        id: ty_id,
        version_req: VersionReq::any(),
      }),
    }],
    dependencies: vec![arora_record_proto::UnfrozenReference {
      id: dep_id,
      version_req: VersionReq::parse(">=0.1,<0.2").unwrap(),
    }],
  };

  let frozen = header.freeze(&reg).unwrap();
  assert_eq!(frozen.dependencies[0].version, v("0.1.0"));
  match &frozen.parameters[0].ty {
    FrozenTy::Scalar(r) => assert_eq!(r.version, v("2.3.0")),
    other => panic!("expected scalar, got {:?}", other),
  }
}

// --- 5. Serde wire-format stability (frozen form serializes/deserializes) ----

#[test]
fn frozen_form_serde_roundtrips() {
  let frozen = FrozenTy::Scalar(arora_record_proto::FrozenReference {
    id: Uuid::nil(),
    version: v("1.2.3"),
  });
  let json = serde_json::to_string(&frozen).unwrap();
  let back: FrozenTy = serde_json::from_str(&json).unwrap();
  assert_eq!(frozen, back);
  assert!(json.contains("\"kind\":\"scalar\""));
}
