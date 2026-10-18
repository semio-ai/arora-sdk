//! A guest module described by its record: the device's registry holds the
//! module record and the records it depends on, and every export — those
//! taking or returning a structure included — joins the method index with
//! the record's frozen signature.
//!
//! The guest is `test-rust-wasm`, whose `reading` returns a `Reading`
//! structure naming a `Quadrant` enumeration and whose `readings` returns an
//! array of them; its header names `Reading` by id alone. Its artifact holds a
//! second module, `compass`, of another scope, whose `bearing` returns a
//! `Reading` too: `Reading` states its scope, `trigonometry`.

use super::*;
use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node, Port};
use arora_behavior_tree::nodes::{PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID};
use arora_behavior_tree::schema::_RET_PARAM_ID;
use arora_registry::local::ROOT_ID;
use arora_registry::local_yaml::write_records_to_yaml_dir;
use arora_registry::{EditableRegistry, TypeDefinitionFrozen};
use arora_types::call::Call;
use arora_types::data::Key;
use arora_types::record::module::frozen::{self, ExportKind};
use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use arora_types::AroraType;
use std::time::Duration;
use test_rust_wasm::test_rust_wasm::{self as declared, Module};
use test_rust_wasm::{compass, Quadrant, Reading, NAVIGATION, TESTS, TRIGONOMETRY};

const WASM: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

fn wasm() -> low::Executor {
    low::Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    }
}

fn header() -> Header {
    declared::header(wasm())
}

/// The version the guest's records are at: its header's.
fn version() -> arora_types::record::Version {
    header().version.into()
}

/// The records a packaged module `M` carries: the module record, in the
/// scope its declaration states, and those of the types its functions name,
/// each in its scope, with the scopes' folders.
fn packaged<M: AroraModule>() -> LocalRegistry {
    let version: arora_types::record::Version = M::header(wasm()).version.into();
    let mut registry = LocalRegistry::new();
    futures::executor::block_on(registry.add_declared_module::<M>(version.0))
        .expect("the module's records are added");
    registry
}

/// The records the packaged guest carries.
fn records() -> LocalRegistry {
    packaged::<Module>()
}

/// A registry holding `module` as the guest's record, and the records of the
/// types the guest names when `with_types`.
fn registry_with(module: frozen::Module, with_types: bool) -> LocalRegistry {
    let mut registry = LocalRegistry::new();
    futures::executor::block_on(async {
        if with_types {
            let records = Module::type_records();
            for scope in records.scopes {
                registry.add_folder(scope.id, scope.record()).await?;
            }
            for (reference, record) in records.types {
                let (id, tag) = (reference.id, reference.version.0);
                match record {
                    TypeDefinitionFrozen::Structure(s) => {
                        registry.add_structure(id, tag, s).await?
                    }
                    TypeDefinitionFrozen::Enumeration(e) => {
                        registry.add_enumeration(id, tag, e).await?
                    }
                    TypeDefinitionFrozen::Primitive(_) => {}
                }
            }
        }
        registry.add_module(Module::id(), version().0, module).await
    })
    .expect("the records are added");
    registry
}

fn device(registry: Option<LocalRegistry>) -> Result<Arora> {
    let builder = Arora::builder().with_module(header(), WASM.to_vec());
    match registry {
        Some(registry) => builder.with_registry(registry),
        None => builder,
    }
    .build()
}

/// A graph calling `reading(0)` and `readings(4)` every step, writing their
/// returns under `trig/reading` and `trig/readings`, and running until
/// halted: what Studio's stepping behavior is to `step_values`.
fn reading_graph() -> Graph {
    let [parallel, reading, readings, run] = [0x1, 0x2, 0x3, 0x4].map(Uuid::from_u128);
    let [reading_out, readings_out] = [0x11, 0x12].map(Uuid::from_u128);
    let mut graph = Graph::empty();
    graph.root = Some(parallel);
    graph.nodes.insert(
        parallel,
        Node {
            id: parallel,
            function: PARALLEL_FUNCTION_ID,
            children: Some(vec![reading, readings, run]),
            ..Node::default()
        },
    );
    graph.nodes.insert(
        reading,
        Node {
            id: reading,
            function: declared::ids::reading::FUNCTION,
            inputs: vec![
                Io::new(declared::ids::reading::ANGLE),
                Io::new(_RET_PARAM_ID),
            ],
            ..Node::default()
        },
    );
    graph.nodes.insert(
        readings,
        Node {
            id: readings,
            function: declared::ids::readings::FUNCTION,
            inputs: vec![
                Io::new(declared::ids::readings::COUNT),
                Io::new(_RET_PARAM_ID),
            ],
            ..Node::default()
        },
    );
    graph.nodes.insert(
        run,
        Node {
            id: run,
            function: RUN_FUNCTION_ID,
            ..Node::default()
        },
    );
    graph.links = vec![
        Link::new(
            Port::new(reading, declared::ids::reading::ANGLE),
            LinkSource::Literal(Value::F32(0.0)),
        ),
        Link::new(
            Port::new(reading, _RET_PARAM_ID),
            LinkSource::Variable(reading_out),
        ),
        Link::new(
            Port::new(readings, declared::ids::readings::COUNT),
            LinkSource::Literal(Value::U32(4)),
        ),
        Link::new(
            Port::new(readings, _RET_PARAM_ID),
            LinkSource::Variable(readings_out),
        ),
    ];
    graph
        .variables
        .insert(reading_out, "trig/reading".to_string());
    graph
        .variables
        .insert(readings_out, "trig/readings".to_string());
    graph
}

fn read(arora: &Arora, key: &str) -> Option<Value> {
    arora.store().read(&[Key::new(key)]).remove(0)
}

/// Without its record, an export returning a structure dispatches but is not
/// described: it is not in the method index, and a behavior calling it fails
/// at the node.
#[test]
fn without_its_record_a_record_typed_export_dispatches_but_is_not_described() {
    let mut arora = device(None).expect("the device builds without records");
    for id in [
        declared::ids::reading::FUNCTION,
        declared::ids::readings::FUNCTION,
    ] {
        assert!(
            !arora.function_index.contains_key(&id),
            "{id} is not described"
        );
    }
    assert!(
        arora
            .function_index
            .contains_key(&declared::ids::cos::FUNCTION),
        "a primitive-typed export is described from the header"
    );

    let called = arora
        .call(Call {
            module_id: Some(Module::id()),
            id: declared::ids::reading::FUNCTION,
            args: vec![StructureField {
                id: declared::ids::reading::ANGLE,
                value: Box::new(Value::F32(0.0)),
            }],
        })
        .expect("reading dispatches");
    assert_eq!(
        Reading::try_from(called.ret).expect("a Reading").quadrant,
        Quadrant::First
    );

    let caller = arora.caller();
    let graph_type = arora_behavior_tree::behavior::graph_type();
    let run = caller_tests::settle(
        &mut arora,
        caller.spawn_graph(&graph_type, &reading_graph()),
    )
    .expect("the graph is spawned");
    arora.step(Duration::from_millis(10)).expect("step");
    assert_eq!(read(&arora, "trig/reading"), None, "the call did not run");
    let failure: Value = arora_behavior::Status::Failure.into();
    assert_eq!(
        arora.store().read(std::slice::from_ref(&run.status)),
        vec![Some(failure)],
        "a node calling an undescribed function fails"
    );
}

/// With its record, every export joins the method index with the record's
/// signature — the one the declaration describes — and `DescribeMethods`
/// lists the record-typed ones with their frozen types.
#[test]
fn a_module_record_describes_every_export_with_its_signature() {
    let mut arora = device(Some(records())).expect("the device builds with its records");
    for declared in Module::exports() {
        let indexed = arora
            .function_index
            .get(&declared.id)
            .unwrap_or_else(|| panic!("{} is described", declared.name));
        assert_eq!(indexed.module_id, Module::id());
        assert_eq!(indexed.function_name, declared.name);
        assert_eq!(indexed.function, declared.signature, "{}", declared.name);
    }

    let caller = arora.caller();
    let methods = caller_tests::settle(
        &mut arora,
        caller.describe_methods(Some("reading".to_string())),
    )
    .expect("the device describes its methods");
    let reading = methods
        .iter()
        .find(|method| method.name == "reading")
        .expect("reading is listed");
    assert_eq!(
        reading.function.return_ty,
        FrozenTy::FrozenScalar(FrozenScalar {
            reference: FrozenReference {
                id: Reading::arora_type_id(),
                version: Reading::arora_type_version(),
            },
        })
    );
    let angle = &reading.function.parameters[&declared::ids::reading::ANGLE];
    assert_eq!(angle.ty, FrozenTy::from(PrimitiveKind::F32));
}

/// A behavior calling the record-typed exports is spawned and ticks: each
/// step, the structure and the array of structures they return land in the
/// store.
#[test]
fn a_behavior_calling_record_typed_exports_runs() {
    let mut arora = device(Some(records())).expect("the device builds with its records");
    let caller = arora.caller();
    let graph_type = arora_behavior_tree::behavior::graph_type();
    let run = caller_tests::settle(
        &mut arora,
        caller.spawn_graph(&graph_type, &reading_graph()),
    )
    .expect("the graph is spawned");
    arora.step(Duration::from_millis(10)).expect("step");

    let running: Value = arora_behavior::Status::Running.into();
    assert_eq!(
        arora.store().read(std::slice::from_ref(&run.status)),
        vec![Some(running)]
    );
    let reading = read(&arora, "trig/reading").expect("the reading is written");
    assert_eq!(
        Reading::try_from(reading).expect("a Reading"),
        Reading::of(0.0)
    );
    let readings = read(&arora, "trig/readings").expect("the readings are written");
    let quadrants: Vec<Quadrant> = readings
        .into_elements()
        .expect("an array")
        .into_iter()
        .map(|reading| Reading::try_from(reading).expect("a Reading").quadrant)
        .collect();
    assert_eq!(
        quadrants,
        vec![
            Quadrant::First,
            Quadrant::Second,
            Quadrant::Third,
            Quadrant::Fourth
        ]
    );
}

/// A record that does not describe its header fails the build, naming the
/// module and the export: a wrong record is a packaging error.
#[test]
fn a_record_that_does_not_match_its_header_fails_the_build_naming_the_export() {
    type Tamper = fn(&mut frozen::Module);
    let cases: [(Tamper, &str); 4] = [
        (
            |module| {
                module
                    .exports
                    .get_mut(&declared::ids::reading::FUNCTION)
                    .unwrap()
                    .name = "read".to_string()
            },
            "export 'reading' (df544fe1-3832-47d8-94e9-233c41867480) is named 'read' in the record",
        ),
        (
            |module| {
                let ExportKind::Function(function) = &mut module
                    .exports
                    .get_mut(&declared::ids::reading::FUNCTION)
                    .unwrap()
                    .kind;
                function.return_ty = FrozenTy::from(PrimitiveKind::F32);
            },
            "export 'reading' (df544fe1-3832-47d8-94e9-233c41867480) returns",
        ),
        (
            |module| {
                let ExportKind::Function(function) = &mut module
                    .exports
                    .get_mut(&declared::ids::readings::FUNCTION)
                    .unwrap()
                    .kind;
                function.parameter_ordering.clear();
            },
            "export 'readings' (bc460484-914b-4be5-b291-bae39b9342a8) has parameters",
        ),
        (
            |module| {
                module.exports.remove(&declared::ids::ping::FUNCTION);
            },
            "export 'ping' (5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6) is not in the record",
        ),
    ];
    for (tamper, expected) in cases {
        let mut record = Module::record(ROOT_ID);
        tamper(&mut record);
        let error = device(Some(registry_with(record, true)))
            .err()
            .expect("a record that does not match fails the build");
        let message = format!("{error:#}");
        assert!(
            message
                .contains("module 'test-rust-wasm' (665d6ec9-3fc9-4cfa-9100-5c8964e95aec@0.1.0)"),
            "{message}"
        );
        assert!(message.contains(expected), "{message}");
    }
}

/// A record whose dependencies the registry lacks fails the build, naming
/// each missing record.
#[test]
fn a_record_whose_dependencies_are_missing_fails_the_build_naming_them() {
    let error = device(Some(registry_with(Module::record(ROOT_ID), false)))
        .err()
        .expect("a record missing its dependencies fails the build");
    let message = format!("{error:#}");
    assert!(
        message.contains("depends on records the device does not hold"),
        "{message}"
    );
    let reading = FrozenReference {
        id: Reading::arora_type_id(),
        version: Reading::arora_type_version(),
    };
    assert!(message.contains(&reading.to_string()), "{message}");
}

/// A fresh device directory for the test `name`.
fn device_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("arora-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Write a module directory `modules/<name>` under `device_dir`: `header`,
/// the guest's artifact and `records` in the records layout.
fn write_module_dir(
    device_dir: &std::path::Path,
    name: &str,
    header: &Header,
    records: &LocalRegistry,
) -> std::path::PathBuf {
    let dir = device_dir.join("modules").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("header.json"),
        serde_json::to_string(header).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("test_rust_wasm.wasm"), WASM).unwrap();
    write_records_to_yaml_dir(records, dir.join("records")).expect("the records are written");
    dir
}

/// The runner's path: a module directory carrying `records/` beside its
/// header and artifact fills the device's registry, which describes the
/// module; records that do not read fail, naming the module.
#[test]
fn a_module_directory_s_records_describe_its_module() {
    let device_dir = device_dir("module-records");
    let dir = write_module_dir(&device_dir, "trigonometry", &header(), &records());

    let modules = module_dir::in_device_dir(&device_dir).expect("the module directory reads");
    let registry = futures::executor::block_on(module_dir::registry(&modules))
        .expect("the module directory's records load");
    let mut builder = Arora::builder().with_registry(registry);
    for module in modules {
        builder = builder.with_module(module.header, module.executable);
    }
    let arora = builder.build().expect("the device builds");
    assert!(arora
        .function_index
        .contains_key(&declared::ids::reading::FUNCTION));

    std::fs::write(
        dir.join("records")
            .join("structure")
            .join(format!("{}@1.1.0.yaml", Reading::arora_type_id())),
        "not: [a structure",
    )
    .unwrap();
    let modules = module_dir::in_device_dir(&device_dir).expect("the module directory reads");
    let error = futures::executor::block_on(module_dir::registry(&modules))
        .err()
        .expect("a record that does not read fails");
    let message = format!("{error:#}");
    assert!(
        message.contains("the records of module 'test-rust-wasm'"),
        "{message}"
    );
    std::fs::remove_dir_all(&device_dir).unwrap();
}

/// Two modules of different scopes sharing `Reading`, which states its
/// scope: their module directories carry the same `Reading` and `Quadrant`
/// records, in `trigonometry`, so they fill one registry and load together
/// on one device, each module in its own scope and each export described.
#[test]
fn two_modules_sharing_a_type_load_together_on_one_device() {
    let device_dir = device_dir("shared-type-records");
    write_module_dir(
        &device_dir,
        "test-rust-wasm",
        &header(),
        &packaged::<Module>(),
    );
    write_module_dir(
        &device_dir,
        "compass",
        &compass::header(wasm()),
        &packaged::<compass::Module>(),
    );

    let modules = module_dir::in_device_dir(&device_dir).expect("the module directories read");
    assert_eq!(modules.len(), 2);
    let registry = futures::executor::block_on(module_dir::registry(&modules))
        .expect("both module directories' records load into one registry");
    let Some(TypeDefinitionFrozen::Structure(reading)) =
        registry.type_at(&Reading::arora_type_id(), &Reading::arora_type_version().0)
    else {
        panic!("Reading's record is held");
    };
    assert_eq!(reading.parent, TRIGONOMETRY.id);
    for (module, scope) in [(Module::id(), TESTS), (compass::Module::id(), NAVIGATION)] {
        let (_, parent) = registry
            .record_name_and_parent(&module)
            .expect("the module's record is held");
        assert_eq!(parent, Some(scope.id));
    }

    let mut builder = Arora::builder().with_registry(registry);
    for module in modules {
        builder = builder.with_module(module.header, module.executable);
    }
    let mut arora = builder
        .build()
        .expect("the device builds with both modules");
    for (module, function) in [
        (Module::id(), declared::ids::reading::FUNCTION),
        (compass::Module::id(), compass::ids::bearing::FUNCTION),
    ] {
        let indexed = arora
            .function_index
            .get(&function)
            .unwrap_or_else(|| panic!("{function} is described"));
        assert_eq!(indexed.module_id, module);
        assert_eq!(
            indexed.function.return_ty,
            FrozenTy::FrozenScalar(FrozenScalar {
                reference: arora_types::record::FrozenReference {
                    id: Reading::arora_type_id(),
                    version: Reading::arora_type_version(),
                },
            })
        );
    }
    let called = arora
        .call(Call {
            module_id: Some(compass::Module::id()),
            id: compass::ids::bearing::FUNCTION,
            args: vec![StructureField {
                id: compass::ids::bearing::DEGREES,
                value: Box::new(Value::F32(100.0)),
            }],
        })
        .expect("bearing dispatches");
    assert_eq!(
        Reading::try_from(called.ret).expect("a Reading").quadrant,
        Quadrant::Second
    );
    std::fs::remove_dir_all(&device_dir).unwrap();
}
