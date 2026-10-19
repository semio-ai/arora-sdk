//! A record whose Rust type is declared elsewhere, through the `module.yaml`
//! path, end to end.
//!
//! The fixture is the behavior `Status` (`arora_behavior::Status`, a
//! `#[derive(AroraType)]` enum) and a `Report` structure that holds one and an
//! array of them, with an export taking a `Status` and returning a `Report`,
//! and an import taking an array of `Status` and returning a `Report`:
//!
//!  * declared in [`ExternTypes`], `Status` is not generated: `Report` and the
//!    export and import shims name `arora_behavior::Status`, and a call
//!    through the export shim reads and writes it;
//!  * not declared, `Status` is generated, and the generated enum and
//!    `arora_behavior::Status` produce identical `Value`s and buffer bytes.

use std::path::PathBuf;
use std::process::Command;

use arora_behavior::{
    declare_status_enumeration, Status, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION,
};
use arora_module_core::analyze_module;
use arora_module_rust::{generate_sources, ExternTypes};
use arora_registry::{
    local::{LocalRegistry, ROOT_ID},
    EditableRegistry,
};
use arora_types::module::high::ModuleDefinition;
use arora_types::record::module::unfrozen::{Export, ExportKind, Function, Module, Parameter};
use arora_types::record::structure::unfrozen::{Structure, StructureField};
use arora_types::record::ty::{UnfrozenArray, UnfrozenScalar, UnfrozenTy};
use arora_types::record::{UnfrozenReference, VersionReq};
use arora_vfs::{Directory, Entry};
use indexmap::IndexMap;
use semver::Version;
use uuid::Uuid;

const REPORT_ID: &str = "e7a1c0de-0000-4000-8000-000000000001";
const REPORT_STATUS_ID: &str = "e7a1c0de-0000-4000-8000-000000000011";
const REPORT_HISTORY_ID: &str = "e7a1c0de-0000-4000-8000-000000000012";
const MODULE_ID: &str = "e7a1c0de-0000-4000-8000-000000000100";
const REPORT_FN_ID: &str = "e7a1c0de-0000-4000-8000-000000000101";
const REPORT_FN_STATUS_ID: &str = "e7a1c0de-0000-4000-8000-000000000102";
const LEDGER_MODULE_ID: &str = "e7a1c0de-0000-4000-8000-000000000200";
const LATEST_FN_ID: &str = "e7a1c0de-0000-4000-8000-000000000103";
const LATEST_FN_STATUS_ID: &str = "e7a1c0de-0000-4000-8000-000000000104";

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn status_reference() -> UnfrozenReference {
    UnfrozenReference {
        id: STATUS_ENUMERATION_ID,
        version_req: VersionReq::any(),
    }
}

async fn build_registry() -> LocalRegistry {
    let mut registry = LocalRegistry::new();
    registry
        .tag_enumeration(
            STATUS_ENUMERATION_ID,
            STATUS_ENUMERATION_VERSION,
            declare_status_enumeration(ROOT_ID),
        )
        .await
        .unwrap();
    let mut fields = IndexMap::new();
    fields.insert(
        id(REPORT_STATUS_ID),
        StructureField {
            name: "status".to_string(),
            ty: UnfrozenTy::UnfrozenScalar(UnfrozenScalar {
                reference: status_reference(),
            }),
        },
    );
    fields.insert(
        id(REPORT_HISTORY_ID),
        StructureField {
            name: "history".to_string(),
            ty: UnfrozenTy::UnfrozenArray(UnfrozenArray {
                reference: status_reference(),
            }),
        },
    );
    registry
        .tag_structure(
            id(REPORT_ID),
            Version::new(1, 0, 0),
            Structure {
                parent: ROOT_ID,
                name: "Report".to_string(),
                fields,
            },
        )
        .await
        .unwrap();

    // The module the fixture imports `latest` from.
    let mut ledger = Module {
        parent: ROOT_ID,
        name: "ledger".to_string(),
        ..Default::default()
    };
    ledger.add_export(
        id(LATEST_FN_ID),
        Export {
            name: "latest".to_string(),
            kind: ExportKind::Function(Function {
                parameters: [(
                    id(LATEST_FN_STATUS_ID),
                    Parameter {
                        name: "status".to_string(),
                        ty: UnfrozenTy::UnfrozenArray(UnfrozenArray {
                            reference: status_reference(),
                        }),
                        mutable: false,
                    },
                )]
                .into_iter()
                .collect(),
                parameter_ordering: vec![id(LATEST_FN_STATUS_ID)],
                return_ty: UnfrozenTy::UnfrozenScalar(UnfrozenScalar {
                    reference: UnfrozenReference {
                        id: id(REPORT_ID),
                        version_req: VersionReq::any(),
                    },
                }),
            }),
        },
    );
    registry
        .tag_module(id(LEDGER_MODULE_ID), Version::new(1, 0, 0), ledger)
        .await
        .unwrap();
    registry
}

fn module_yaml() -> String {
    format!(
        r#"
id: {MODULE_ID}
name: reporter
author: Semio
description: A record with a declared Rust type through the generator
license: Apache-2.0
version: {{ major: 0, minor: 1, patch: 0 }}
executor:
  name: wasm
exports:
  - type: function
    id: {REPORT_FN_ID}
    name: report
    parameters:
      - id: {REPORT_FN_STATUS_ID}
        name: status
        type: {{ kind: scalar, id: {STATUS_ENUMERATION_ID} }}
    ret: {{ kind: scalar, id: {REPORT_ID} }}
imports:
  - type: function
    module: {LEDGER_MODULE_ID}
    id: {LATEST_FN_ID}
    name: latest
    parameters:
      - id: {LATEST_FN_STATUS_ID}
        name: status
        type: {{ kind: array, id: {STATUS_ENUMERATION_ID} }}
    ret: {{ kind: scalar, id: {REPORT_ID} }}
executable_mime: application/wasm
"#
    )
}

async fn generate(extern_types: &ExternTypes) -> Directory {
    let mut registry = build_registry().await;
    let definition: ModuleDefinition =
        serde_yaml::from_str(&module_yaml()).expect("the fixture header is valid");
    let assets = analyze_module(definition, &mut registry)
        .await
        .expect("analyze_module");
    generate_sources(assets, &mut registry, extern_types)
        .await
        .expect("generate_sources")
}

fn file_text(directory: &Directory, name: &str) -> Option<String> {
    match directory.get(name)? {
        Entry::File(file) => Some(String::from_utf8_lossy(&file.data).into_owned()),
        Entry::Directory(_) => None,
    }
}

/// Writes `generated` and `main_rs` into a throwaway crate, runs it, and
/// checks it printed `marker`.
async fn run_fixture(name: &str, generated: &Directory, main_rs: &str, marker: &str) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let crates_dir = manifest_dir.join("../..").canonicalize().unwrap();
    let dir = std::env::temp_dir().join(format!("arora-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.0.0"
edition = "2021"

[workspace]

[dependencies]
arora-behavior = {{ path = "{behavior}" }}
arora-buffers = {{ path = "{buffers}" }}
arora-types = {{ path = "{types}" }}
derive_more = {{ version = "2", features = ["display"] }}
uuid = {{ version = "1", features = ["v4"] }}
"#,
        behavior = crates_dir.join("arora-behavior").display(),
        buffers = crates_dir.join("arora-buffers").display(),
        types = crates_dir.join("arora-types").display(),
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
    std::fs::write(dir.join("src/main.rs"), main_rs).unwrap();
    generated
        .sync(dir.join("src/arora_generated"))
        .await
        .expect("write the generated sources");
    let output = Command::new(env!("CARGO"))
        .args(["run", "--quiet"])
        .current_dir(&dir)
        .output()
        .expect("invoke cargo");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains(marker),
        "the {name} fixture failed to compile or run.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_declared_record_is_referenced_not_generated() {
    let mut extern_types = ExternTypes::new();
    extern_types.declare::<Status>("arora_behavior::Status");
    let generated = generate(&extern_types).await;

    assert!(
        file_text(&generated, "status.rs").is_none(),
        "a declared record must not be generated"
    );
    let report = file_text(&generated, "report.rs").expect("report.rs");
    assert!(
        report.contains("arora_behavior :: Status"),
        "a field of a declared record names its declared type:\n{report}"
    );
    let import = file_text(&generated, "ledger.rs").expect("ledger.rs");
    assert!(
        import.contains("arora_behavior :: Status"),
        "an import shim names the declared type:\n{import}"
    );

    let shim = format!("arora_function_{}", REPORT_FN_ID.replace('-', "_"));
    let main_rs = format!(
        r#"
pub mod arora_generated;

use arora_behavior::Status;
use arora_generated::report::Report;
use arora_types::value::{{Structure, StructureField, Value}};
use uuid::Uuid;

pub fn report(status: Option<Status>) -> Report {{
    let status = status.expect("status");
    Report {{ status, history: vec![Status::Running, status] }}
}}

fn main() {{
    let arg = arora_buffers::froto_value::serialize(&Value::Structure(Structure {{
        id: Uuid::parse_str("{REPORT_FN_ID}").unwrap(),
        fields: vec![StructureField {{
            id: Uuid::parse_str("{REPORT_FN_STATUS_ID}").unwrap(),
            value: Box::new(Status::Failure.into()),
        }}],
    }}));
    let result = arora_generated::export::{shim}(arg.as_ptr() as usize) as *const u8;
    let size = u32::from_le_bytes(unsafe {{ *(result as *const [u8; 4]) }}) as usize;
    let buffer = unsafe {{ std::slice::from_raw_parts(result, size) }};
    let Value::Structure(result) = arora_buffers::froto_value::deserialize(buffer) else {{
        panic!("expected a result structure");
    }};
    let returned = *result.fields.into_iter().next().expect("a return field").value;
    assert_eq!(
        Report::try_from(returned).expect("a Report"),
        Report {{ status: Status::Failure, history: vec![Status::Running, Status::Failure] }}
    );
    println!("EXTERN_OK");
}}
"#
    );
    run_fixture("extern-types-fixture", &generated, &main_rs, "EXTERN_OK").await;
}

#[tokio::test]
async fn a_generated_record_matches_its_declared_type() {
    let generated = generate(&ExternTypes::new()).await;
    assert!(file_text(&generated, "status.rs").is_some());

    let main_rs = r#"
pub mod arora_generated;

use arora_types::value::Value;

pub fn report(_status: Option<arora_generated::status::Status>) -> arora_generated::report::Report {
    unreachable!("only the types are exercised")
}

fn main() {
    use arora_generated::status::Status as Generated;
    for (generated, declared) in [
        (Generated::Success, arora_behavior::Status::Success),
        (Generated::Failure, arora_behavior::Status::Failure),
        (Generated::Running, arora_behavior::Status::Running),
    ] {
        let generated: Value = generated.into();
        let declared: Value = declared.into();
        assert_eq!(generated, declared, "the generated Status and arora_behavior's disagree");
        assert_eq!(
            arora_buffers::froto_value::serialize(&generated),
            arora_buffers::froto_value::serialize(&declared),
        );
        assert_eq!(arora_behavior::Status::try_from(generated).unwrap(), arora_behavior::Status::try_from(declared).unwrap());
    }
    println!("GOLDEN_OK");
}
"#;
    run_fixture("generated-twin-fixture", &generated, main_rs, "GOLDEN_OK").await;
}
