//! The C++ guests and the Rust guests are the same kind of WebAssembly module:
//! a core module, built with the same target features, importing only the
//! engine's `env` functions and WASI preview 1, and exporting the Arora ABI
//! without a `_start`. A guest that diverges — a C++ guest left behind when
//! the Rust guests move to another target, or one linked as a WASI command —
//! fails here.

use std::collections::BTreeSet;
use std::path::PathBuf;

use wasmparser::{BinaryReader, Encoding, Parser, Payload};

/// The module the engine's own imports (`arora_dispatch`, …) come from.
const ENGINE: &str = "env";
/// The module WASI preview 1's imports come from.
const WASI_P1: &str = "wasi_snapshot_preview1";

/// What a guest is, as far as the form the guests share goes.
#[derive(Debug, PartialEq)]
struct Form {
    encoding: Encoding,
    /// The modules its imports come from, the engine's left out.
    system_interfaces: BTreeSet<String>,
    /// Its `target_features` section, e.g. `+bulk-memory`.
    target_features: BTreeSet<String>,
}

/// A guest, read: its form and the names it exports.
struct Guest {
    name: &'static str,
    form: Form,
    exports: BTreeSet<String>,
}

fn read(name: &'static str, path: PathBuf) -> Guest {
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut encoding = None;
    let mut system_interfaces = BTreeSet::new();
    let mut target_features = BTreeSet::new();
    let mut exports = BTreeSet::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload.unwrap_or_else(|e| panic!("{name}: {e}")) {
            Payload::Version {
                encoding: found, ..
            } => {
                encoding.get_or_insert(found);
            }
            Payload::ImportSection(imports) => {
                for import in imports.into_imports() {
                    let import = import.unwrap();
                    if import.module != ENGINE {
                        system_interfaces.insert(import.module.to_string());
                    }
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    exports.insert(export.unwrap().name.to_string());
                }
            }
            Payload::CustomSection(section) if section.name() == "target_features" => {
                let mut reader = BinaryReader::new(section.data(), 0);
                for _ in 0..reader.read_var_u32().unwrap() {
                    let prefix = reader.read_u8().unwrap() as char;
                    let feature = reader.read_string().unwrap();
                    target_features.insert(format!("{prefix}{feature}"));
                }
            }
            _ => {}
        }
    }
    Guest {
        name,
        form: Form {
            encoding: encoding.expect("a version header"),
            system_interfaces,
            target_features,
        },
        exports,
    }
}

/// Where the C++ modules' build scripts write their module directories.
fn module_dir(name: &str) -> PathBuf {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root")
        .join("target")
        .join(profile)
        .join("modules")
        .join(name)
}

#[test]
fn the_cpp_and_rust_guests_have_one_form() {
    let rust = [
        read(
            "test-rust-wasm",
            env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm").into(),
        ),
        read(
            "animation",
            env!("CARGO_CDYLIB_FILE_ARORA_ANIMATION_MODULE_arora_animation_module").into(),
        ),
    ];
    let cpp = ["test-cpp", "test-cpp-2", "example-cpp"]
        .map(|name| read(name, module_dir(name).join(format!("{name}.wasm"))));

    let reference = &rust[0];
    assert_eq!(reference.form.encoding, Encoding::Module);
    assert_eq!(
        reference.form.system_interfaces,
        BTreeSet::from([WASI_P1.to_string()])
    );
    for guest in rust.iter().chain(&cpp) {
        assert_eq!(guest.form, reference.form, "{}", guest.name);
        for abi in ["memory", "arora_buffer_alloc", "arora_buffer_free"] {
            assert!(guest.exports.contains(abi), "{} exports {abi}", guest.name);
        }
        assert!(
            guest
                .exports
                .iter()
                .any(|name| name.starts_with("arora_function_")),
            "{} exports its functions",
            guest.name
        );
        assert!(
            !guest.exports.contains("_start"),
            "{} is not a WASI command",
            guest.name
        );
    }
}
