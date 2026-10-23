//! The C++ guests and the Rust guests are the same kind of WebAssembly module:
//! a component of the `arora:module` world, exporting its `functions` and
//! `dispatch` and importing only interfaces every Arora host provides, laid
//! out as every host runs one — one guest core module, built with the same
//! target features, without a `_start`, exporting what the canonical ABI
//! calls (`cabi_realloc`, and `_initialize` when the component initializes
//! it). A guest that diverges — one left behind when the others move to
//! another target, one importing an interface a host does not provide, one
//! linked as a WASI command, one carrying modules the browser does not run —
//! fails here.

use std::collections::BTreeSet;
use std::path::PathBuf;

use arora_engine::component_layout::{guest_module, provided};
use wasmparser::{BinaryReader, ComponentTypeRef, Encoding, Parser, Payload};

/// What a guest is, as far as the form the guests share goes.
#[derive(Debug, PartialEq)]
struct Form {
    encoding: Encoding,
    /// The component's exports.
    exports: BTreeSet<String>,
    /// The guest core module's `target_features` section, e.g. `+bulk-memory`.
    target_features: BTreeSet<String>,
}

/// A guest, read: its form, the component's imports, the guest core module's
/// exports, and whether the component initializes the guest.
struct Guest {
    name: &'static str,
    form: Form,
    /// Each import's name, and whether it is an instance (an interface).
    imports: Vec<(String, bool)>,
    core_exports: BTreeSet<String>,
    initialize: bool,
}

fn read(name: &'static str, path: PathBuf) -> Guest {
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    // The layout every host runs, and the guest core module in it.
    let guest = guest_module(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));

    // The component's own sections; its core modules' come after, and are
    // matched by none of these.
    let mut encoding = None;
    let (mut exports, mut imports) = (BTreeSet::new(), Vec::new());
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload.unwrap_or_else(|e| panic!("{name}: {e}")) {
            Payload::Version {
                encoding: found, ..
            } => {
                encoding.get_or_insert(found);
            }
            Payload::ComponentImportSection(section) => {
                for import in section {
                    let import = import.unwrap();
                    let interface = matches!(import.ty, ComponentTypeRef::Instance(_));
                    imports.push((import.name.name.to_string(), interface));
                }
            }
            Payload::ComponentExportSection(section) => {
                for export in section {
                    exports.insert(export.unwrap().name.name.to_string());
                }
            }
            _ => {}
        }
    }

    let (mut core_exports, mut target_features) = (BTreeSet::new(), BTreeSet::new());
    for payload in Parser::new(0).parse_all(guest.bytes) {
        match payload.unwrap_or_else(|e| panic!("{name}: {e}")) {
            Payload::ExportSection(section) => {
                for export in section {
                    core_exports.insert(export.unwrap().name.to_string());
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
            exports,
            target_features,
        },
        imports,
        core_exports,
        initialize: guest.initialize,
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
            "test-rust-component",
            env!("CARGO_CDYLIB_FILE_TEST_RUST_COMPONENT_test_rust_component").into(),
        ),
        read(
            "animation",
            env!("CARGO_CDYLIB_FILE_ARORA_ANIMATION_MODULE_arora_animation_module").into(),
        ),
        read(
            "test-behavior-tree-nodes",
            env!("CARGO_CDYLIB_FILE_TEST_BEHAVIOR_TREE_NODES_test_behavior_tree_nodes").into(),
        ),
    ];
    let cpp = ["test-cpp", "test-cpp-2", "example-cpp"]
        .map(|name| read(name, module_dir(name).join(format!("{name}.wasm"))));

    let reference = &rust[0];
    assert_eq!(reference.form.encoding, Encoding::Component);
    assert!(
        !reference.form.target_features.is_empty(),
        "the guest module names its target features"
    );
    assert_eq!(
        reference.form.exports,
        BTreeSet::from(["dispatch".to_string(), "functions".to_string()])
    );
    for guest in rust.iter().chain(&cpp) {
        assert_eq!(guest.form, reference.form, "{}", guest.name);
        for (import, interface) in &guest.imports {
            // An import other than an interface is a type the world names
            // (`id`), which needs nothing from a host.
            assert!(
                !interface || provided(import),
                "{} imports {import}, which not every Arora host provides",
                guest.name
            );
        }
        assert!(
            guest.core_exports.contains("cabi_realloc"),
            "{} exports the allocator the canonical ABI calls",
            guest.name
        );
        assert_eq!(
            guest.core_exports.contains("_initialize"),
            guest.initialize,
            "{}: the component initializes the guest exactly when it exports _initialize",
            guest.name
        );
        assert!(
            !guest.core_exports.contains("_start"),
            "{} is not a WASI command",
            guest.name
        );
    }
}
