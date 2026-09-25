//! Optional types through the `module.yaml` path, end to end.
//!
//! A hand-written header declares optional parameters, an optional return and
//! a structure with optional fields. The generated Rust is compiled into a
//! throwaway crate and run natively:
//!
//!  * the structure round-trips through `Value` and through buffers, and its
//!    bytes match the `serde_uuid` codec;
//!  * the export shim is called with an optional argument absent, present,
//!    explicitly `None`, and bare (refused);
//!  * the `module.yaml` the generator writes declares the optionals, and reads
//!    back to the same frozen signature.

use std::path::{Path, PathBuf};
use std::process::Command;

use arora_module_core::header::module_frozen_from_header_file;
use arora_module_core::{analyze_module, ModuleAsset};
use arora_module_rust::generate_sources;
use arora_registry::{local::LocalRegistry, EditableRegistry};
use arora_types::module::high::ModuleDefinition;
use arora_types::record::module::frozen::ExportKind;
use arora_types::record::structure::frozen::{
    Structure as FrozenStructure, StructureField as FrozenStructureField,
};
use arora_types::record::structure::unfrozen::{Structure, StructureField};
use arora_types::record::ty::{FrozenOption, FrozenScalar, FrozenTy, PrimitiveKind, UnfrozenTy};
use arora_types::record::FrozenReference;
use indexmap::IndexMap;
use semver::Version;
use uuid::Uuid;

const COORD_ID: &str = "ac5edad8-7a7e-4611-a4bd-80ce7dcfc33d";
const MAYBE_ID: &str = "74144a67-af34-4026-aced-23c1ecc6a176";
const MODULE_ID: &str = "86f945bd-3a71-4084-8216-ee155304f0f4";
const SAMPLE_ID: &str = "d10d04d3-8eae-4152-b9de-c98dff96bd57";
const SAMPLE_ANIM_ID: &str = "45c3bb36-0804-4986-b215-532c2e9a78ef";
const SAMPLE_RATE_ID: &str = "9f955964-b119-460a-bd3d-02c74a987714";
const SAMPLE_WINDOW_ID: &str = "80a26ce4-fd3f-4c06-b58a-a3ea62151f98";
const ECHO_ID: &str = "dcb7e227-4188-4644-a6c7-d298fbeb6528";
const ECHO_MAYBE_ID: &str = "f60d8d88-810b-421e-9204-aa4b19864c42";

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn optional(element: FrozenTy) -> FrozenTy {
    FrozenTy::FrozenOption(FrozenOption {
        element: Box::new(element),
    })
}

fn coord_ty() -> FrozenTy {
    FrozenTy::FrozenScalar(FrozenScalar {
        reference: FrozenReference {
            id: id(COORD_ID),
            version: arora_types::record::Version(Version::new(1, 0, 0)),
        },
    })
}

async fn build_registry() -> LocalRegistry {
    let root = arora_registry::local::ROOT_ID;
    let mut registry = LocalRegistry::new();

    // Coord { x, y: f32 }.
    let mut coord_fields = IndexMap::new();
    for name in ["x", "y"] {
        coord_fields.insert(
            Uuid::new_v4(),
            StructureField {
                name: name.to_string(),
                ty: UnfrozenTy::from(PrimitiveKind::F32),
            },
        );
    }
    registry
        .tag_structure(
            id(COORD_ID),
            Version::new(1, 0, 0),
            Structure {
                parent: root,
                name: "Coord".to_string(),
                fields: coord_fields,
            },
        )
        .await
        .unwrap();

    // Maybe { rate: Option<f32>, coord: Option<Coord> }.
    let mut maybe_fields = IndexMap::new();
    maybe_fields.insert(
        Uuid::new_v4(),
        FrozenStructureField {
            name: "rate".to_string(),
            ty: optional(FrozenTy::from(PrimitiveKind::F32)),
        },
    );
    maybe_fields.insert(
        Uuid::new_v4(),
        FrozenStructureField {
            name: "coord".to_string(),
            ty: optional(coord_ty()),
        },
    );
    registry
        .add_structure(
            id(MAYBE_ID),
            Version::new(1, 0, 0),
            FrozenStructure {
                parent: root,
                name: "Maybe".to_string(),
                fields: maybe_fields,
            },
        )
        .await
        .unwrap();

    registry
}

fn module_yaml() -> String {
    format!(
        r#"
id: {MODULE_ID}
name: sampler
author: Semio
description: Optional types through the generator
license: Proprietary
version: {{ major: 0, minor: 1, patch: 0 }}
executor:
  name: wasm
exports:
  - type: function
    id: {SAMPLE_ID}
    name: sample
    parameters:
      - id: {SAMPLE_ANIM_ID}
        name: anim
        type: {{ kind: scalar, id: u32 }}
      - id: {SAMPLE_RATE_ID}
        name: rate
        type: {{ kind: option, id: f32 }}
      - id: {SAMPLE_WINDOW_ID}
        name: window
        type: {{ kind: option, id: {COORD_ID} }}
    ret: {{ kind: option, id: str }}
  - type: function
    id: {ECHO_ID}
    name: echo
    parameters:
      - id: {ECHO_MAYBE_ID}
        name: maybe
        type: {{ kind: scalar, id: {MAYBE_ID} }}
    ret: {{ kind: scalar, id: {MAYBE_ID} }}
imports: []
executable_mime: application/wasm
"#
    )
}

fn write_fixture_crate(dir: &Path) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let crates_dir = manifest_dir.join("../..").canonicalize().unwrap();
    let cargo_toml = format!(
        r#"[package]
name = "optional-fixture"
version = "0.0.0"
edition = "2021"

[workspace]

[dependencies]
arora-buffers = {{ path = "{buffers}" }}
arora-types = {{ path = "{types}" }}
derive_more = {{ version = "2", features = ["display"] }}
uuid = {{ version = "1", features = ["v4"] }}
"#,
        buffers = crates_dir.join("arora-buffers").display(),
        types = crates_dir.join("arora-types").display(),
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();

    let sample_shim = format!("arora_function_{}", SAMPLE_ID.replace('-', "_"));
    let echo_shim = format!("arora_function_{}", ECHO_ID.replace('-', "_"));
    let main_rs = format!(
        r#"
pub mod arora_generated;

use arora_generated::coord::Coord;
use arora_generated::maybe::Maybe;
use arora_types::value::{{Structure, StructureField, Value}};
use uuid::Uuid;

// The functions the generated export shims call. Every parameter reaches a
// function as an `Option` (absent is `None`); an optional one already is.
pub fn sample(anim: Option<u32>, rate: Option<f32>, window: Option<Coord>) -> Option<String> {{
    let anim = anim?;
    let rate = rate.unwrap_or(60.0);
    Some(match window {{
        Some(window) => format!("{{anim}} at {{rate}} from {{}}", window.x),
        None => format!("{{anim}} at {{rate}}"),
    }})
}}

pub fn echo(maybe: Option<Maybe>) -> Maybe {{
    maybe.expect("maybe")
}}

fn id(s: &str) -> Uuid {{
    Uuid::parse_str(s).unwrap()
}}

fn field(field_id: &str, value: Value) -> StructureField {{
    StructureField {{ id: id(field_id), value: Box::new(value) }}
}}

// Call an export shim the way an executor does: a size-prefixed buffer in,
// one out, decoded by the generic `Value` codec.
fn call(shim: extern "C" fn(usize) -> usize, function: &str, fields: Vec<StructureField>) -> Value {{
    try_call(shim, function, fields).unwrap_or_else(|e| panic!("{{function}} failed: {{e}}"))
}}

fn try_call(shim: extern "C" fn(usize) -> usize, function: &str, fields: Vec<StructureField>) -> Result<Value, String> {{
    let arg = arora_buffers::serde_uuid::serialize(&Value::Structure(Structure {{
        id: id(function),
        fields,
    }}));
    let result = shim(arg.as_ptr() as usize) as *const u8;
    let size = u32::from_le_bytes(unsafe {{ *(result as *const [u8; 4]) }}) as usize;
    let buffer = unsafe {{ std::slice::from_raw_parts(result, size) }};
    if buffer[4] == arora_buffers::TYPE_ERROR {{
        return Err(String::from_utf8_lossy(&buffer[5..]).into_owned());
    }}
    match arora_buffers::serde_uuid::deserialize(buffer) {{
        Value::Structure(s) => Ok(*s.fields.into_iter().next().expect("a return field").value),
        other => panic!("expected a result structure, got {{other:?}}"),
    }}
}}

fn some_string(s: &str) -> Value {{
    Value::Option(Some(Box::new(Value::String(s.to_string()))))
}}

fn main() {{
    // A structure with optional fields, present and absent.
    for maybe in [
        Maybe {{ rate: Some(2.5), coord: Some(Coord {{ x: 1.0, y: 2.0 }}) }},
        Maybe {{ rate: None, coord: None }},
    ] {{
        let value: Value = maybe.clone().into();
        assert_eq!(Maybe::try_from(value.clone()).expect("from Value"), maybe);
        let bytes: Box<[u8]> = maybe.clone().into();
        assert_eq!(Maybe::try_from(&bytes[..]).expect("from bytes"), maybe);
        assert_eq!(
            &bytes[..],
            &arora_buffers::serde_uuid::serialize(&value)[..],
            "the generated codec and serde_uuid disagree on optionals"
        );
        let echoed = call(arora_generated::export::{echo_shim}, "{ECHO_ID}", vec![field("{ECHO_MAYBE_ID}", value.clone())]);
        assert_eq!(echoed, value, "an optional field crosses the shim");
    }}

    let sample = arora_generated::export::{sample_shim};
    let anim = || field("{SAMPLE_ANIM_ID}", Value::U32(3));
    let rate = |value| field("{SAMPLE_RATE_ID}", value);
    assert_eq!(call(sample, "{SAMPLE_ID}", vec![anim()]), some_string("3 at 60"), "absent");
    assert_eq!(
        call(sample, "{SAMPLE_ID}", vec![anim(), rate(Value::Option(Some(Box::new(Value::F32(30.0)))))]),
        some_string("3 at 30"),
        "present"
    );
    assert!(
        try_call(sample, "{SAMPLE_ID}", vec![anim(), rate(Value::F32(30.0))]).is_err(),
        "a bare element is not an optional"
    );
    assert_eq!(call(sample, "{SAMPLE_ID}", vec![anim(), rate(Value::Option(None))]), some_string("3 at 60"), "explicit None");
    let window = Value::Option(Some(Box::new(Coord {{ x: 4.0, y: 0.0 }}.into())));
    assert_eq!(
        call(sample, "{SAMPLE_ID}", vec![anim(), field("{SAMPLE_WINDOW_ID}", window)]),
        some_string("3 at 60 from 4"),
        "an optional structure"
    );
    assert_eq!(call(sample, "{SAMPLE_ID}", vec![]), Value::Option(None), "an empty optional return");
    println!("OPTIONAL_OK");
}}
"#
    );
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), main_rs).unwrap();
}

#[tokio::test]
async fn optionals_generate_compile_run_and_export() {
    let mut registry = build_registry().await;
    let definition: ModuleDefinition =
        serde_yaml::from_str(&module_yaml()).expect("the fixture header is valid");
    let assets = analyze_module(definition, &mut registry)
        .await
        .expect("analyze_module");

    // The resolved signature carries the optional forms.
    let declared = assets
        .iter()
        .find_map(|asset| match asset {
            ModuleAsset::Module(_, _, module, _) => Some(module.clone()),
            _ => None,
        })
        .expect("the module asset");
    let ExportKind::Function(sample) = &declared.exports[&id(SAMPLE_ID)].kind;
    assert_eq!(
        sample.parameters[&id(SAMPLE_RATE_ID)].ty,
        optional(FrozenTy::from(PrimitiveKind::F32))
    );
    assert_eq!(
        sample.parameters[&id(SAMPLE_WINDOW_ID)].ty,
        optional(coord_ty())
    );
    assert_eq!(
        sample.return_ty,
        optional(FrozenTy::from(PrimitiveKind::String))
    );

    let generated = generate_sources(assets, &mut registry)
        .await
        .expect("generate_sources");
    let dir = std::env::temp_dir().join(format!("arora-optional-fixture-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    write_fixture_crate(&dir);
    generated
        .sync(dir.join("src/arora_generated"))
        .await
        .expect("write the generated sources");

    // The written module.yaml declares the optionals and reads back to the
    // same frozen signature.
    let header_path = dir.join("src/arora_generated/module.yaml");
    let header = std::fs::read_to_string(&header_path).expect("module.yaml");
    assert!(header.contains("kind: option"), "{header}");
    let (_, _, read_back) = module_frozen_from_header_file(&header_path, &mut registry)
        .await
        .expect("the written header reads back");
    let ExportKind::Function(read_sample) = &read_back.module.exports[&id(SAMPLE_ID)].kind;
    assert_eq!(read_sample.parameters, sample.parameters);
    assert_eq!(read_sample.return_ty, sample.return_ty);

    let output = Command::new(env!("CARGO"))
        .args(["run", "--quiet"])
        .current_dir(&dir)
        .output()
        .expect("invoke cargo");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("OPTIONAL_OK"),
        "the generated fixture failed to compile or run.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
