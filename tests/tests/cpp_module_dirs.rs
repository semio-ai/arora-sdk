//! The C++ guests ship as module directories, the form the Rust guests ship
//! in: a device given them loads them, describes their exports from their
//! records, and answers calls into them.

use std::collections::HashSet;
use std::path::PathBuf;

use arora_types::call::Call;
use arora_types::value::{Structure, StructureField, Value};
use uuid::Uuid;

/// Where the C++ modules' build scripts write their module directories.
fn modules_dir() -> PathBuf {
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
}

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("a valid uuid")
}

/// A device given test-cpp's and test-cpp-2's module directories describes
/// every export from the modules' records — `test_structured`, whose second
/// parameter is a structure only test-cpp-2's `records/` holds, included —
/// and answers calls into both, test-cpp-2's own call into test-cpp included.
#[test]
fn a_device_given_the_cpp_module_directories_describes_and_calls_them() {
    let modules = modules_dir();
    let dirs = [modules.join("test-cpp"), modules.join("test-cpp-2")];
    let device_dir = std::env::temp_dir().join(format!(
        "arora-cpp-module-directories-{}",
        std::process::id()
    ));
    let mut device =
        futures::executor::block_on(arora::Arora::builder().with_module_dirs(&device_dir, &dirs))
            .expect("the module directories and their records load")
            .build()
            .expect("the records describe the headers");

    let methods = device.describe_methods(None);
    let method = |name: &str| {
        methods
            .iter()
            .find(|method| method.name == name)
            .unwrap_or_else(|| panic!("{name} is described"))
    };
    assert_eq!(
        method("test_optional").id,
        id("a24fd5ab-baf8-4f44-af41-053a54b3fa81")
    );
    assert_eq!(
        method("test_2_args").id,
        id("b213a552-77ad-465a-a26d-352e8eccfd63")
    );
    let test_structured = method("test_structured");
    let structure =
        &test_structured.function.parameters[&id("63086e48-804f-403a-8862-3358ddedc08d")];
    let mut references = HashSet::new();
    structure.ty.dependencies(&mut references);
    assert!(
        references
            .iter()
            .any(|reference| reference.id == id("7f9aedf8-dbde-4020-b5f4-c28a6635ae7c")),
        "test_structured's `b` is the structure test-cpp-2's records declare: {:?}",
        structure.ty
    );

    let field = |field: &str, value: Value| StructureField {
        id: id(field),
        value: Box::new(value),
    };
    let optional = device
        .call(Call {
            module_id: Some(id("23a75559-025c-4be8-8b48-0784e30dc9ec")),
            id: id("a24fd5ab-baf8-4f44-af41-053a54b3fa81"),
            args: vec![field(
                "b3ec8dd2-2df1-43ae-bf2d-0a567c998243",
                Value::Option(Some(Box::new(Value::U32(41)))),
            )],
        })
        .expect("test_optional answers");
    assert_eq!(optional.ret, Value::Option(Some(Box::new(Value::U32(42)))));

    let structured = device
        .call(Call {
            module_id: Some(id("325c5e47-32db-4e23-a38f-7a2849647e0c")),
            id: id("07f5740c-ba4a-45af-8ec5-bedde5737e99"),
            args: vec![
                field("b41899c3-66dc-40d4-ab61-d1ccf5231c88", Value::Boolean(true)),
                field(
                    "63086e48-804f-403a-8862-3358ddedc08d",
                    Value::Structure(Structure {
                        id: id("7f9aedf8-dbde-4020-b5f4-c28a6635ae7c"),
                        fields: vec![
                            field("7d94a956-e50d-4cc4-9714-f62e1f9b134e", Value::Boolean(true)),
                            field("5ffa9104-1e5c-4026-943f-8db38bd34563", Value::I32(113)),
                        ],
                    }),
                ),
            ],
        })
        .expect("test_structured answers");
    assert_eq!(structured.ret, Value::I32(1));
}

/// The example C++ module answers its calls, and its state lives across them:
/// its global vector of sums was constructed once, when the device loaded the
/// module, and every call since appended to it.
#[test]
fn the_example_cpp_module_keeps_its_state_across_calls() {
    const MODULE: &str = "e8e2d89e-d4c7-440d-8683-0b5ec6c5407e";
    let dirs = [modules_dir().join("example-cpp")];
    let device_dir = std::env::temp_dir().join(format!(
        "arora-example-cpp-module-directory-{}",
        std::process::id()
    ));
    let mut device =
        futures::executor::block_on(arora::Arora::builder().with_module_dirs(&device_dir, &dirs))
            .expect("the module directory and its records load")
            .build()
            .expect("the record describes the header");

    let mut add = |a: i32, b: i32| {
        device
            .call(Call {
                module_id: Some(id(MODULE)),
                id: id("4a3020dc-6db3-43c2-bb75-6a7db237329a"),
                args: vec![
                    StructureField {
                        id: id("e15b0627-29df-441c-bea8-706ddc92f712"),
                        value: Box::new(Value::I32(a)),
                    },
                    StructureField {
                        id: id("0374610f-5f4a-41d8-97f3-64f48cf21f40"),
                        value: Box::new(Value::I32(b)),
                    },
                ],
            })
            .expect("add_two_integers answers")
            .ret
    };
    assert_eq!(add(2, 3), Value::I64(5));
    assert_eq!(add(i32::MAX, i32::MAX), Value::I64(2 * i64::from(i32::MAX)));

    let sums = device
        .call(Call {
            module_id: Some(id(MODULE)),
            id: id("26c0cfc2-da81-4561-a67c-af7133a478a5"),
            args: vec![],
        })
        .expect("sums_so_far answers");
    assert_eq!(sums.ret, Value::U32(2));
}

/// test-cpp loaded from its module directory into an engine of its own.
fn test_cpp_engine() -> (std::pin::Pin<Box<arora_engine::engine::Engine>>, Uuid) {
    let dir = modules_dir().join("test-cpp");
    let header: arora_types::module::low::Header =
        serde_json::from_str(&std::fs::read_to_string(dir.join("header.json")).unwrap()).unwrap();
    let module = header.id;
    let guest = std::fs::read(dir.join("test-cpp.wasm")).unwrap();
    let mut engine = arora_engine::engine::EngineBuilder::new()
        .add_executor(arora_engine::executor::component::ComponentExecutor::new().unwrap())
        .build();
    arora_engine::load::load_module_from_parts(&mut engine, header, guest.into()).unwrap();
    (engine, module)
}

/// A C++ component refuses, as its error, an argument that is not the Arora
/// Buffer its size prefix counts — an empty one, one whose prefix counts more
/// than it holds — and goes on answering calls.
#[test]
fn a_cpp_component_refuses_an_argument_that_is_not_an_arora_buffer() {
    use arora_engine::call::CallBridge;
    let (mut engine, module) = test_cpp_engine();
    let test_optional = id("a24fd5ab-baf8-4f44-af41-053a54b3fa81");
    let argument = StructureField {
        id: id("b3ec8dd2-2df1-43ae-bf2d-0a567c998243"),
        value: Box::new(Value::U32(41)),
    };
    let truncated = cut_short(
        &Value::Structure(Structure {
            id: test_optional,
            fields: vec![argument.clone()],
        }),
        3,
    );
    for arg in [
        &[][..],
        &[0xff, 0xff, 0, 0, 0x0b],
        &[2, 0, 0, 0],
        &truncated,
    ] {
        let error = engine.dispatch(&module, &test_optional, arg).unwrap_err();
        assert!(
            matches!(&error, arora_engine::module::DispatchError::Guest { message }
                if message.contains("not an Arora Buffer")),
            "{arg:?}: {error:?}"
        );
    }
    let result = engine
        .arora_call(Call {
            module_id: Some(module),
            id: test_optional,
            args: vec![argument],
        })
        .expect("a call after the refusals");
    assert_eq!(
        result.ret,
        Value::Option(Some(Box::new(Value::U32(42)))),
        "test_optional increments its argument"
    );
}

/// A C++ guest reads an argument as its declared type whatever it carries:
/// the engine refuses one not of its parameter's type, or naming a parameter
/// the function does not declare, before the guest reads it, and the module
/// answers next.
#[test]
fn a_cpp_component_is_never_given_an_argument_of_another_type() {
    use arora_engine::call::CallBridge;
    let (mut engine, module) = test_cpp_engine();
    let field = |i: &str, value: Value| StructureField {
        id: id(i),
        value: Box::new(value),
    };
    let test_2_args = id("b213a552-77ad-465a-a26d-352e8eccfd63");
    let test_optional = id("a24fd5ab-baf8-4f44-af41-053a54b3fa81");
    const A: &str = "55dbec70-1c3a-433e-a6e6-27446b7f065e";
    const B: &str = "abf9ca4e-e03f-431a-a32b-4911f809c399";
    const OPTIONAL: &str = "b3ec8dd2-2df1-43ae-bf2d-0a567c998243";
    for (function, args, named) in [
        (
            test_2_args,
            vec![field(A, Value::String("x".into())), field(B, Value::U32(1))],
            "'a'",
        ),
        (
            test_2_args,
            vec![field(A, Value::F64(1.0)), field(B, Value::U32(1))],
            "'a'",
        ),
        (
            test_optional,
            vec![field(OPTIONAL, Value::String("x".into()))],
            "'a'",
        ),
        (
            test_optional,
            vec![field(
                OPTIONAL,
                Value::Option(Some(Box::new(Value::String("x".into())))),
            )],
            "'a'",
        ),
        (
            test_optional,
            vec![field(B, Value::U32(1))],
            "has no parameter",
        ),
    ] {
        let refused = engine
            .arora_call(Call {
                module_id: Some(module),
                id: function,
                args,
            })
            .unwrap_err();
        assert!(
            matches!(&refused, arora_engine::call::CallError::Generic { message } if message.contains(named)),
            "{refused:?}"
        );
    }
    let result = engine
        .arora_call(Call {
            module_id: Some(module),
            id: test_optional,
            args: vec![field(OPTIONAL, Value::U32(41))],
        })
        .expect("a call after the refusals");
    assert_eq!(result.ret, Value::Option(Some(Box::new(Value::U32(42)))));
}

/// The Arora Buffer of `value`, cut `by` bytes short, its size prefix counting
/// what is left: a prefix that holds, and a value that does not.
fn cut_short(value: &Value, by: usize) -> Vec<u8> {
    let whole = arora_buffers::serde_uuid::serialize(value);
    let mut cut = whole[..whole.len() - by].to_vec();
    let len = cut.len() as u32;
    cut[..4].copy_from_slice(&len.to_le_bytes());
    cut
}
