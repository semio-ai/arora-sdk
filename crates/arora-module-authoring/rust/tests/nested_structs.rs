//! End-to-end regression test for nested / recursive / dynamic struct codegen.
//!
//! It authors a fully-typed fixture module whose schema exercises:
//!
//!  * a transitive reference chain `Track` -> `Keypoint` -> `Coord` / a leaf
//!    enumeration, where only `Track` is named directly by an export (so the
//!    generator must transitively emit `Keypoint`, `Coord` and the enum);
//!  * a `Vec<Keypoint>` (array-of-struct) field, whose serialization used to be
//!    malformed and never compiled;
//!  * `Tree { children: Vec<Tree> }`, a recursive type reached through a `Vec`
//!    (cycle-safe transitive walk, no `Box` needed, constructible);
//!  * `Cons { tail: Cons }`, a scalar self-reference that must be `Box`-wrapped
//!    to have finite size (compile-only — no base case to construct); and
//!  * `Dynamic { payload: Value }`, a "dynamic value" escape-hatch field
//!    generated as the raw runtime `Value` rather than a declared struct.
//!
//! The generated Rust is written to a throwaway crate that is actually compiled
//! and run, round-tripping values through both `Into<Value>` / `TryFrom<Value>`
//! and the binary buffer `Into<Box<[u8]>>` / `TryFrom<&[u8]>` conversions.

use std::path::{Path, PathBuf};
use std::process::Command;

use arora_module_core::{analyze_module, ModuleAsset};
use arora_module_rust::{
    generate_common_sources, generate_enumeration_source, generate_mods_in_directories,
    generate_structure_source,
};
use arora_registry::{
    local::LocalRegistry, EditableRegistry, ReadableRegistry, TypeDefinitionFrozen,
};
use arora_types::module::high::ModuleDefinition;
use arora_types::record::structure::frozen::{
    Structure as FrozenStructure, StructureField as FrozenStructureField,
};
use arora_types::record::structure::unfrozen::{Structure, StructureField};
use arora_types::record::ty::{
    FrozenArray, FrozenScalar, FrozenTy, PrimitiveKind, UnfrozenArray, UnfrozenScalar, UnfrozenTy,
};
use arora_types::record::{
    enumeration::unfrozen::{Enumeration, EnumerationVariant},
    FrozenReference, UnfrozenReference, VersionReq,
};
use indexmap::IndexMap;
use semver::Version;
use uuid::Uuid;

const COORD_ID: &str = "11111111-1111-4111-8111-111111111111";
const INTERP_ID: &str = "22222222-2222-4222-8222-222222222222";
const KEYPOINT_ID: &str = "33333333-3333-4333-8333-333333333333";
const TRACK_ID: &str = "44444444-4444-4444-8444-444444444444";
// Self-referential (recursive) and dynamic-value fixture types.
const TREE_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DYNAMIC_ID: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const CONS_ID: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn rec_version() -> arora_types::record::Version {
    arora_types::record::Version(Version::new(1, 0, 0))
}

fn frozen_scalar(type_id: Uuid) -> FrozenTy {
    FrozenTy::FrozenScalar(FrozenScalar {
        reference: FrozenReference {
            id: type_id,
            version: rec_version(),
        },
    })
}

fn frozen_array(type_id: Uuid) -> FrozenTy {
    FrozenTy::FrozenArray(FrozenArray {
        reference: FrozenReference {
            id: type_id,
            version: rec_version(),
        },
    })
}

fn frozen_field(name: &str, ty: FrozenTy) -> FrozenStructureField {
    FrozenStructureField {
        name: name.to_string(),
        ty,
    }
}

/// A frozen structure with the given fields, parented at the registry root.
fn frozen_struct(name: &str, fields: Vec<(Uuid, FrozenStructureField)>) -> FrozenStructure {
    let mut map = IndexMap::new();
    for (fid, f) in fields {
        map.insert(fid, f);
    }
    FrozenStructure {
        parent: arora_registry::local::ROOT_ID,
        name: name.to_string(),
        fields: map,
    }
}

fn primitive(kind: PrimitiveKind) -> UnfrozenTy {
    UnfrozenTy::from(kind)
}

fn scalar_ref(type_id: &str) -> UnfrozenTy {
    UnfrozenTy::UnfrozenScalar(UnfrozenScalar {
        reference: UnfrozenReference {
            id: id(type_id),
            version_req: VersionReq::any(),
        },
    })
}

fn array_ref(type_id: &str) -> UnfrozenTy {
    UnfrozenTy::UnfrozenArray(UnfrozenArray {
        reference: UnfrozenReference {
            id: id(type_id),
            version_req: VersionReq::any(),
        },
    })
}

fn field(name: &str, ty: UnfrozenTy) -> StructureField {
    StructureField {
        name: name.to_string(),
        ty,
    }
}

async fn build_registry() -> LocalRegistry {
    let root = arora_registry::local::ROOT_ID;
    let mut registry = LocalRegistry::new();
    let v1 = Version::new(1, 0, 0);

    // Coord { x, y, z: f32 } — the leaf struct at the bottom of the chain.
    let mut coord_fields = IndexMap::new();
    coord_fields.insert(Uuid::new_v4(), field("x", primitive(PrimitiveKind::F32)));
    coord_fields.insert(Uuid::new_v4(), field("y", primitive(PrimitiveKind::F32)));
    coord_fields.insert(Uuid::new_v4(), field("z", primitive(PrimitiveKind::F32)));
    registry
        .tag_structure(
            id(COORD_ID),
            v1.clone(),
            Structure {
                parent: root,
                name: "Coord".to_string(),
                fields: coord_fields,
            },
        )
        .await
        .unwrap();

    // Interpolation { Linear, Step } — a leaf enumeration.
    let mut interp_variants = IndexMap::new();
    interp_variants.insert(
        Uuid::new_v4(),
        EnumerationVariant {
            name: "Linear".to_string(),
            ty: primitive(PrimitiveKind::Unit),
        },
    );
    interp_variants.insert(
        Uuid::new_v4(),
        EnumerationVariant {
            name: "Step".to_string(),
            ty: primitive(PrimitiveKind::Unit),
        },
    );
    registry
        .tag_enumeration(
            id(INTERP_ID),
            v1.clone(),
            Enumeration {
                parent: root,
                name: "Interpolation".to_string(),
                variants: interp_variants,
            },
        )
        .await
        .unwrap();

    // Keypoint { time: f32, position: Coord, interpolation: Interpolation }.
    let mut keypoint_fields = IndexMap::new();
    keypoint_fields.insert(Uuid::new_v4(), field("time", primitive(PrimitiveKind::F32)));
    keypoint_fields.insert(Uuid::new_v4(), field("position", scalar_ref(COORD_ID)));
    keypoint_fields.insert(
        Uuid::new_v4(),
        field("interpolation", scalar_ref(INTERP_ID)),
    );
    registry
        .tag_structure(
            id(KEYPOINT_ID),
            v1.clone(),
            Structure {
                parent: root,
                name: "Keypoint".to_string(),
                fields: keypoint_fields,
            },
        )
        .await
        .unwrap();

    // Track { name: String, points: Vec<Keypoint>, modes: Vec<Interpolation> } —
    // exercises both an array-of-struct (`points`) and an array-of-enum
    // (`modes`) field in one value.
    let mut track_fields = IndexMap::new();
    track_fields.insert(
        Uuid::new_v4(),
        field("name", primitive(PrimitiveKind::String)),
    );
    track_fields.insert(Uuid::new_v4(), field("points", array_ref(KEYPOINT_ID)));
    track_fields.insert(Uuid::new_v4(), field("modes", array_ref(INTERP_ID)));
    registry
        .tag_structure(
            id(TRACK_ID),
            v1.clone(),
            Structure {
                parent: root,
                name: "Track".to_string(),
                fields: track_fields,
            },
        )
        .await
        .unwrap();

    // Recursive / dynamic types are added *frozen* (their references would not
    // resolve while freezing a self-referential value). All are parented at the
    // registry root.
    //
    // Tree { label: String, children: Vec<Tree> } — recursive through a `Vec`,
    // so no `Box` is needed and it is actually constructible.
    registry
        .add_structure(
            id(TREE_ID),
            v1.clone(),
            frozen_struct(
                "Tree",
                vec![
                    (
                        Uuid::new_v4(),
                        frozen_field("label", FrozenTy::from(PrimitiveKind::String)),
                    ),
                    (
                        Uuid::new_v4(),
                        frozen_field("children", frozen_array(id(TREE_ID))),
                    ),
                ],
            ),
        )
        .await
        .unwrap();

    // Dynamic { tag: String, payload: <KEY_VALUE_ID> } — the `payload` field is a
    // dynamic value escape hatch, generated as the raw `Value` type.
    registry
        .add_structure(
            id(DYNAMIC_ID),
            v1.clone(),
            frozen_struct(
                "Dynamic",
                vec![
                    (
                        Uuid::new_v4(),
                        frozen_field("tag", FrozenTy::from(PrimitiveKind::String)),
                    ),
                    (
                        Uuid::new_v4(),
                        frozen_field("payload", frozen_scalar(*arora_types::ty::KEY_VALUE_ID)),
                    ),
                ],
            ),
        )
        .await
        .unwrap();

    // Cons { head: i32, tail: Cons } — a *scalar* self-reference. It must be
    // `Box`-wrapped to have finite size (compile-only: it has no base case and
    // so cannot be constructed without an optional/nullable field).
    registry
        .add_structure(
            id(CONS_ID),
            v1.clone(),
            frozen_struct(
                "Cons",
                vec![
                    (
                        Uuid::new_v4(),
                        frozen_field("head", FrozenTy::from(PrimitiveKind::I32)),
                    ),
                    (
                        Uuid::new_v4(),
                        frozen_field("tail", frozen_scalar(id(CONS_ID))),
                    ),
                ],
            ),
        )
        .await
        .unwrap();

    registry
}

fn fixture_module_yaml() -> String {
    // A single export whose parameter and return type are `Track`. Nothing here
    // names `Keypoint`, `Coord` or `Interpolation`: the generator must reach
    // them by walking `Track`'s fields.
    format!(
        r#"
id: 55555555-5555-4555-8555-555555555555
name: animation
author: Semio
description: Fixture animation module
license: Proprietary
version:
  major: 0
  minor: 1
  patch: 0
executor:
  name: wasm
exports:
  - type: function
    id: 66666666-6666-4666-8666-666666666666
    name: process
    parameters:
      - id: 77777777-7777-4777-8777-777777777777
        name: track
        type:
          kind: scalar
          id: {track}
      - id: 88888888-8888-4888-8888-888888888888
        name: tree
        type:
          kind: scalar
          id: {tree}
      - id: 99999999-9999-4999-8999-999999999999
        name: dynamic
        type:
          kind: scalar
          id: {dynamic}
      - id: aaaaaaa1-0000-4000-8000-000000000001
        name: cons
        type:
          kind: scalar
          id: {cons}
    ret:
      kind: scalar
      id: {track}
imports: []
executable_mime: application/wasm
"#,
        track = TRACK_ID,
        tree = TREE_ID,
        dynamic = DYNAMIC_ID,
        cons = CONS_ID,
    )
}

fn write_fixture_crate(dir: &Path) {
    // Absolute paths to the workspace crates the generated code depends on.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let crates_dir = manifest_dir.join("../..").canonicalize().unwrap();
    let arora_buffers = crates_dir.join("arora-buffers");
    let arora_types = crates_dir.join("arora-types");

    let cargo_toml = format!(
        r#"[package]
name = "nested-structs-fixture"
version = "0.0.0"
edition = "2021"

[workspace]

[dependencies]
arora-buffers = {{ path = "{buffers}" }}
arora-types = {{ path = "{types}" }}
derive_more = {{ version = "2", features = ["display"] }}
uuid = {{ version = "1", features = ["v4"] }}

[[bin]]
name = "roundtrip"
path = "src/main.rs"
"#,
        buffers = arora_buffers.display(),
        types = arora_types.display(),
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();

    // The user-authored driver: constructs values and round-trips them through
    // both conversions. `Cons` is referenced only for its type (a scalar
    // self-reference cannot be constructed without a base case), proving the
    // `Box`-wrapped recursive type at least compiles.
    let main_rs = r#"
pub mod arora_generated;

use arora_generated::coord::Coord;
use arora_generated::interpolation::Interpolation;
use arora_generated::keypoint::Keypoint;
use arora_generated::track::Track;
use arora_generated::tree::Tree;
use arora_generated::dynamic::Dynamic;
use arora_types::value::Value;

// Referencing the type ensures `cons.rs` (a `Box`-wrapped scalar self-reference)
// is compiled.
#[allow(dead_code)]
type ConsAlias = arora_generated::cons::Cons;

fn sample_track() -> Track {
    Track {
        name: "wave".to_string(),
        points: vec![
            Keypoint {
                time: 0.0,
                position: Coord { x: 1.0, y: 2.0, z: 3.0 },
                interpolation: Interpolation::Linear,
            },
            Keypoint {
                time: 1.5,
                position: Coord { x: 4.0, y: 5.0, z: 6.0 },
                interpolation: Interpolation::Step,
            },
        ],
        modes: vec![Interpolation::Step, Interpolation::Linear, Interpolation::Step],
    }
}

fn sample_tree() -> Tree {
    Tree {
        label: "root".to_string(),
        children: vec![
            Tree { label: "a".to_string(), children: vec![] },
            Tree {
                label: "b".to_string(),
                children: vec![Tree { label: "b1".to_string(), children: vec![] }],
            },
        ],
    }
}

fn sample_dynamic() -> Dynamic {
    Dynamic {
        tag: "meta".to_string(),
        // An open value whose leaves are a mix of primitives, an array and a
        // key/value map — none of which are declared structs.
        payload: Value::ArrayValue(vec![
            Value::F32(2.5),
            Value::Boolean(true),
            Value::String("free-form".to_string()),
        ]),
    }
}

fn round_trip<T>(value: T, what: &str)
where
    T: Clone + PartialEq + std::fmt::Debug + Into<Value> + Into<Box<[u8]>>
        + for<'a> TryFrom<&'a [u8]> + TryFrom<Value>,
    <T as TryFrom<Value>>::Error: std::fmt::Debug,
    for<'a> <T as TryFrom<&'a [u8]>>::Error: std::fmt::Debug,
{
    let as_value: Value = value.clone().into();
    let from_value = T::try_from(as_value).expect("Value round-trip failed");
    assert_eq!(value, from_value, "{} Value round-trip mismatch", what);

    let bytes: Box<[u8]> = value.clone().into();
    let from_bytes = T::try_from(&bytes[..]).expect("buffer round-trip failed");
    assert_eq!(value, from_bytes, "{} buffer round-trip mismatch", what);
}

fn track_has_array_structure(v: &Value) -> bool {
    matches!(v, Value::Structure(s)
        if s.fields.iter().any(|f| matches!(f.value.as_ref(), Value::ArrayStructure { .. })))
}

fn track_has_array_enumeration(v: &Value) -> bool {
    matches!(v, Value::Structure(s)
        if s.fields.iter().any(|f| matches!(f.value.as_ref(), Value::ArrayEnumeration { .. })))
}

// Buffer-level cross-boundary conformance: the bytes the generated codegen
// writes for an array-of-struct (`points`) and an array-of-enum (`modes`) MUST
// be exactly what `arora_buffers::serde_uuid` (the generic `Value` codec that
// marshals the `arora_call` boundary) reads and writes, BOTH directions. Guards
// `serde_uuid <-> generated-codegen`, not just codegen<->codegen.
fn cross_boundary_buffer() {
    let track = sample_track();

    // (1) generated codegen ENCODES -> serde_uuid (generic Value codec) DECODES.
    let gen_bytes: Box<[u8]> = track.clone().into();
    let decoded: Value = arora_buffers::serde_uuid::deserialize(&gen_bytes);

    // Elements must decode in the raw layout: `ArrayStructure` / `ArrayEnumeration`
    // (the old full-element encoding would misparse here).
    assert!(
        track_has_array_structure(&decoded),
        "buffer: `points` did not decode via serde_uuid as an ArrayStructure: {decoded:?}"
    );
    assert!(
        track_has_array_enumeration(&decoded),
        "buffer: `modes` did not decode via serde_uuid as an ArrayEnumeration: {decoded:?}"
    );

    // (2) serde_uuid RE-ENCODES; it must be byte-identical to the generated one.
    let su_bytes = arora_buffers::serde_uuid::serialize(&decoded);
    assert_eq!(
        &gen_bytes[..],
        &su_bytes[..],
        "generated codegen and serde_uuid disagree on array-of-struct/enum wire bytes"
    );

    // (3) generated codegen DECODES serde_uuid's bytes back to the value.
    let from_serde =
        Track::try_from(&su_bytes[..]).expect("generated codegen failed to decode serde_uuid bytes");
    assert_eq!(
        track, from_serde,
        "serde_uuid -> generated codegen buffer round-trip mismatch"
    );
}

// Value-level cross-boundary conformance: `Into<Value>` must produce the typed
// `ArrayStructure` / `ArrayEnumeration` (NOT `ArrayValue`), so that encoding
// *that* `Value` through serde_uuid yields the same bytes as the direct buffer
// path. Closes the `Track -> Value -> serde_uuid` route, and `TryFrom<Value>`
// round-trips it.
fn cross_boundary_value() {
    let track = sample_track();

    let value: Value = track.clone().into();
    assert!(
        track_has_array_structure(&value),
        "Value: `points` must be ArrayStructure (not ArrayValue): {value:?}"
    );
    assert!(
        track_has_array_enumeration(&value),
        "Value: `modes` must be ArrayEnumeration (not ArrayValue): {value:?}"
    );

    let value_bytes = arora_buffers::serde_uuid::serialize(&value);
    let buffer_bytes: Box<[u8]> = track.clone().into();
    assert_eq!(
        &value_bytes[..],
        &buffer_bytes[..],
        "Track -> Value -> serde_uuid bytes differ from the direct buffer path"
    );

    let back = Track::try_from(value).expect("Value cross-boundary round-trip failed");
    assert_eq!(track, back, "Value cross-boundary round-trip mismatch");
}

fn main() {
    round_trip(sample_track(), "Track (array-of-struct + array-of-enum + nested)");
    round_trip(sample_tree(), "Tree (recursive via Vec<Self>)");
    round_trip(sample_dynamic(), "Dynamic (raw Value field)");
    cross_boundary_buffer();
    cross_boundary_value();
    println!("ROUNDTRIP_OK");
}
"#;
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), main_rs).unwrap();
}

#[tokio::test]
async fn nested_and_array_of_struct_generate_compile_and_round_trip() {
    let mut registry = build_registry().await;

    let module_definition: ModuleDefinition =
        serde_yaml::from_str(&fixture_module_yaml()).expect("fixture module YAML is valid");

    let assets = analyze_module(module_definition, &mut registry)
        .await
        .expect("analyze_module failed");

    // Bug 1: every transitively-referenced type must be emitted, not just the
    // export-referenced `Track`.
    let emitted: Vec<String> = assets
        .iter()
        .filter_map(|asset| match asset {
            ModuleAsset::Type(_, _, type_def) => Some(match type_def {
                arora_registry::TypeDefinitionFrozen::Structure(s) => s.name.clone(),
                arora_registry::TypeDefinitionFrozen::Enumeration(e) => e.name.clone(),
                arora_registry::TypeDefinitionFrozen::Primitive(_) => "<primitive>".to_string(),
            }),
            _ => None,
        })
        .collect();
    // `Keypoint`, `Coord` and `Interpolation` are only reachable transitively
    // through `Track`; `Tree`/`Dynamic`/`Cons` are named directly but exercise
    // recursion and the dynamic escape hatch.
    for expected in [
        "Track",
        "Keypoint",
        "Coord",
        "Interpolation",
        "Tree",
        "Dynamic",
        "Cons",
    ] {
        assert!(
            emitted.iter().any(|name| name == expected),
            "referenced type `{expected}` was not emitted; got {emitted:?}"
        );
    }

    // Build the `arora_generated` module tree from the type assets. We
    // deliberately assemble only the type modules (plus the shared `error.rs`)
    // here: this test targets the struct/enum code generation exercised by the
    // two bugs, so it skips the export-handler wrappers.
    let mut generated = generate_common_sources().expect("generate_common_sources failed");
    for asset in &assets {
        let ModuleAsset::Type(type_id, _, type_def) = asset else {
            continue;
        };
        let sources = match type_def {
            TypeDefinitionFrozen::Structure(structure) => {
                let parent_path = registry.resolve_id(&structure.parent).await.unwrap();
                generate_structure_source(type_id, structure, &mut registry, &parent_path)
                    .await
                    .expect("generate_structure_source failed")
            }
            TypeDefinitionFrozen::Enumeration(enumeration) => {
                let parent_path = registry.resolve_id(&enumeration.parent).await.unwrap();
                generate_enumeration_source(type_id, enumeration, &parent_path)
                    .expect("generate_enumeration_source failed")
            }
            TypeDefinitionFrozen::Primitive(_) => continue,
        };
        generated = generated.merge_with(&sources);
    }
    generate_mods_in_directories(&mut generated).expect("generate_mods_in_directories failed");

    // Write the generated tree into a throwaway crate and compile + run it.
    let dir = std::env::temp_dir().join(format!("arora-nested-fixture-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    write_fixture_crate(&dir);
    generated
        .sync(dir.join("src/arora_generated"))
        .await
        .expect("failed to write generated sources");

    let output = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--bin", "roundtrip"])
        .current_dir(&dir)
        .output()
        .expect("failed to invoke cargo");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("ROUNDTRIP_OK"),
        "generated fixture crate failed to compile or round-trip.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );

    // Only clean up on success so failures can be inspected.
    let _ = std::fs::remove_dir_all(&dir);
}
