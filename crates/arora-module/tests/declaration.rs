//! A module declared in Rust is the same module everywhere: the ids a header
//! carries, the record a store serves, the functions a host dispatches, the
//! stubs a caller programs against.

use arora_behavior::Status;
use arora_engine::engine::{EngineBuilder, PinnedEngine};
use arora_engine::module::HostModule;
use arora_module::AroraModule;
use arora_types::call::{Call, CallBridge};
use arora_types::module::low::{Executor, ExportSymbol, TypeRef};
use arora_types::record::module::frozen::ExportKind;
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use arora_types::{AroraType, Uuid};

/// A point: the boundary type an export takes and returns.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "7c1d6f2e-0a3b-4c5d-8e9f-000000000001")]
pub struct Point {
    #[arora(id = "7c1d6f2e-0a3b-4c5d-8e9f-000000000011")]
    x: f64,
    #[arora(id = "7c1d6f2e-0a3b-4c5d-8e9f-000000000012")]
    y: f64,
}

#[arora_module::module(
    id = "11112222-3333-4444-8555-000000000001",
    name = "geometry",
    version = "0.2.0",
    author = "Semio",
    license = "MIT",
    description = "A declared module under test",
    executable_mime = "application/wasm"
)]
pub mod geometry {
    use super::{Point, Status};

    /// Takes a string, returns an enumeration.
    #[export(id = "11112222-3333-4444-8555-000000000010")]
    pub fn say(#[param(id = "11112222-3333-4444-8555-000000000011")] text: String) -> Status {
        if text.is_empty() {
            Status::Failure
        } else {
            Status::Success
        }
    }

    /// Takes two structures and an array, returns a structure.
    #[export(id = "11112222-3333-4444-8555-000000000020")]
    pub fn midpoint(
        #[param(id = "11112222-3333-4444-8555-000000000021")] a: Point,
        #[param(id = "11112222-3333-4444-8555-000000000022")] b: Point,
    ) -> Point {
        Point {
            x: (a.x + b.x) / 2.0,
            y: (a.y + b.y) / 2.0,
        }
    }

    /// Writes through a mutable parameter, returns an array.
    #[export(id = "11112222-3333-4444-8555-000000000030")]
    pub fn scale(
        #[param(id = "11112222-3333-4444-8555-000000000031")] factors: Vec<f32>,
        #[param(id = "11112222-3333-4444-8555-000000000032")] applied: &mut String,
    ) -> Vec<f32> {
        *applied = format!("{} factors", factors.len());
        factors.into_iter().map(|f| f * 2.0).collect()
    }
}

fn engine() -> PinnedEngine {
    let mut engine = EngineBuilder::new().build();
    let module = HostModule::of::<geometry::Module>();
    engine.register_module(module.id(), Box::new(module));
    engine
}

fn executor() -> Executor {
    Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    }
}

#[test]
fn the_declaration_is_the_source_of_every_id() {
    assert_eq!(
        geometry::ids::MODULE,
        Uuid::parse_str("11112222-3333-4444-8555-000000000001").unwrap()
    );
    assert_eq!(
        geometry::ids::say::FUNCTION,
        Uuid::parse_str("11112222-3333-4444-8555-000000000010").unwrap()
    );
    assert_eq!(
        geometry::ids::say::TEXT,
        Uuid::parse_str("11112222-3333-4444-8555-000000000011").unwrap()
    );
    assert_eq!(geometry::Module::id(), geometry::ids::MODULE);
}

#[test]
fn the_header_declares_every_export_with_its_parameters() {
    let header = geometry::Module::header(executor());
    assert_eq!(header.id, geometry::ids::MODULE);
    assert_eq!(header.name, "geometry");
    assert_eq!(header.version.minor, 2);
    assert_eq!(header.executor.name, "wasm");
    assert_eq!(header.exports.len(), 3);

    let ExportSymbol::Function(say) = header
        .exports
        .iter()
        .find(|ExportSymbol::Function(f)| f.id == geometry::ids::say::FUNCTION)
        .expect("say is exported");
    assert_eq!(say.name, "say");
    assert_eq!(say.parameters.len(), 1);
    assert_eq!(say.parameters[0].id, geometry::ids::say::TEXT);
    assert_eq!(say.parameters[0].name, "text");
    assert!(!say.parameters[0].mutable);
    assert!(matches!(
      say.parameters[0].ty,
      TypeRef::Scalar { id } if id == *arora_types::ty::STRING_ID
    ));
    assert!(matches!(say.ret, TypeRef::Scalar { id } if id == Status::arora_type_id()));

    let ExportSymbol::Function(scale) = header
        .exports
        .iter()
        .find(|ExportSymbol::Function(f)| f.id == geometry::ids::scale::FUNCTION)
        .expect("scale is exported");
    assert!(scale.parameters[1].mutable, "`&mut` is a mutable parameter");
    assert!(matches!(
      scale.parameters[0].ty,
      TypeRef::Array { id } if id == *arora_types::ty::F32_ID
    ));
}

#[test]
fn the_declaration_is_also_the_store_record() {
    let parent = Uuid::parse_str("11112222-3333-4444-8555-0000000000ff").unwrap();
    let record = geometry::Module::record(parent);
    assert_eq!(record.parent, parent);
    assert_eq!(record.name, "geometry");
    assert_eq!(record.exports.len(), 3);

    let midpoint = record
        .exports
        .get(&geometry::ids::midpoint::FUNCTION)
        .expect("midpoint is a record export");
    assert_eq!(midpoint.name, "midpoint");
    let ExportKind::Function(signature) = &midpoint.kind;
    assert_eq!(signature.parameter_ordering.len(), 2);
    assert_eq!(
        signature.parameter_ordering[0],
        geometry::ids::midpoint::A,
        "parameters keep their declared order"
    );

    // A record names the versioned types its signatures depend on.
    let point = record
        .dependencies
        .iter()
        .find(|reference| reference.id == Point::arora_type_id())
        .expect("Point is a dependency");
    assert_eq!(point.version, Point::arora_type_version());
    assert!(
        record
            .dependencies
            .iter()
            .filter(|reference| reference.id == Point::arora_type_id())
            .count()
            == 1,
        "a type used twice is named once"
    );

    // The record round-trips through the store's wire form.
    let yaml = serde_yaml::to_string(&record).expect("a record serializes");
    let read: arora_types::record::module::frozen::Module =
        serde_yaml::from_str(&yaml).expect("a record reads back");
    assert_eq!(read.exports.len(), record.exports.len());
}

#[test]
fn a_frozen_signature_pins_its_types_at_a_version() {
    let record = geometry::Module::record(Uuid::nil());
    let ExportKind::Function(say) = &record
        .exports
        .get(&geometry::ids::say::FUNCTION)
        .expect("say")
        .kind;
    let FrozenTy::FrozenScalar(scalar) = &say.return_ty else {
        panic!("Status is a scalar reference, got {:?}", say.return_ty);
    };
    assert_eq!(scalar.reference.id, Status::arora_type_id());
    assert_eq!(scalar.reference.version, Status::arora_type_version());

    let ExportKind::Function(scale) = &record
        .exports
        .get(&geometry::ids::scale::FUNCTION)
        .expect("scale")
        .kind;
    assert_eq!(
        scale.return_ty,
        FrozenTy::from(PrimitiveKind::ArrayF32),
        "an array of a primitive is that primitive's array kind"
    );
}

#[test]
fn a_declared_module_dispatches_through_the_engine_by_parameter_id() {
    let mut engine = engine();
    let result = engine
        .arora_call(Call {
            module_id: Some(geometry::ids::MODULE),
            id: geometry::ids::say::FUNCTION,
            args: vec![StructureField {
                id: geometry::ids::say::TEXT,
                value: Box::new(Value::String("hello".into())),
            }],
        })
        .expect("say dispatches");
    assert_eq!(
        Status::try_from(result.ret).expect("a Status"),
        Status::Success
    );
    assert!(result.mutated.is_empty());
}

#[test]
fn arguments_are_matched_by_id_not_by_position() {
    let mut engine = engine();
    let a = StructureField {
        id: geometry::ids::midpoint::A,
        value: Box::new(Value::from(Point { x: 0.0, y: 0.0 })),
    };
    let b = StructureField {
        id: geometry::ids::midpoint::B,
        value: Box::new(Value::from(Point { x: 4.0, y: 2.0 })),
    };
    let call = |args: Vec<StructureField>| Call {
        module_id: Some(geometry::ids::MODULE),
        id: geometry::ids::midpoint::FUNCTION,
        args,
    };
    let declared = engine
        .arora_call(call(vec![a.clone(), b.clone()]))
        .expect("midpoint dispatches");
    let reversed = engine
        .arora_call(call(vec![b, a]))
        .expect("midpoint dispatches with its arguments the other way round");
    assert_eq!(
        Point::try_from(declared.ret).unwrap(),
        Point { x: 2.0, y: 1.0 }
    );
    assert_eq!(
        Point::try_from(reversed.ret).unwrap(),
        Point { x: 2.0, y: 1.0 },
        "the order the caller sends its arguments in does not change the call"
    );
}

#[test]
fn a_mutable_parameter_comes_back_under_its_id() {
    let mut engine = engine();
    let result = engine
        .arora_call(Call {
            module_id: Some(geometry::ids::MODULE),
            id: geometry::ids::scale::FUNCTION,
            args: vec![
                StructureField {
                    id: geometry::ids::scale::FACTORS,
                    value: Box::new(Value::ArrayF32(vec![1.0, 2.0])),
                },
                StructureField {
                    id: geometry::ids::scale::APPLIED,
                    value: Box::new(Value::String(String::new())),
                },
            ],
        })
        .expect("scale dispatches");
    assert_eq!(result.ret, Value::ArrayF32(vec![2.0, 4.0]));
    let applied = result
        .mutated
        .iter()
        .find(|field| field.id == geometry::ids::scale::APPLIED)
        .expect("the mutated parameter comes back under its id");
    assert_eq!(*applied.value, Value::String("2 factors".into()));
}

#[test]
fn a_missing_parameter_fails_the_call_by_name() {
    let mut engine = engine();
    let error = engine
        .arora_call(Call {
            module_id: Some(geometry::ids::MODULE),
            id: geometry::ids::say::FUNCTION,
            args: vec![],
        })
        .expect_err("say without its text");
    assert!(
        error
            .to_string()
            .contains("missing parameter `text` of `say`"),
        "{error}"
    );
}

#[test]
fn an_argument_of_the_wrong_type_fails_the_call() {
    let mut engine = engine();
    let error = engine
        .arora_call(Call {
            module_id: Some(geometry::ids::MODULE),
            id: geometry::ids::say::FUNCTION,
            args: vec![StructureField {
                id: geometry::ids::say::TEXT,
                value: Box::new(Value::U32(7)),
            }],
        })
        .expect_err("say with a number");
    assert!(
        error.to_string().contains("parameter `text` of `say`"),
        "{error}"
    );
}

#[test]
fn the_declaration_is_its_own_client_interface() {
    let mut engine = engine();
    let status = geometry::client::say(&mut engine, "hello".to_string()).expect("the stub calls");
    assert_eq!(status, Status::Success);

    let mut applied = String::new();
    let scaled =
        geometry::client::scale(&mut engine, vec![3.0], &mut applied).expect("the stub calls");
    assert_eq!(scaled, vec![6.0]);
    assert_eq!(
        applied, "1 factors",
        "the stub reads the mutated parameter back"
    );
}

#[test]
fn every_export_is_described_for_method_introspection() {
    let module = HostModule::of::<geometry::Module>();
    let described: Vec<&str> = module
        .descriptions()
        .iter()
        .map(|description| description.name.as_str())
        .collect();
    assert_eq!(described.len(), 3);
    assert!(described.contains(&"say"));
    assert!(described.contains(&"midpoint"));
    assert!(described.contains(&"scale"));
}

#[test]
fn the_artifact_entry_point_speaks_the_engine_wire_format() {
    // What an executor calls in a built artifact: a size-prefixed argument
    // buffer in, a size-prefixed result buffer out, both in the `Value` wire
    // format. Here the symbol is called directly, so the marshalling is under
    // test without an executor to load a binary.
    use arora_types::value::{Structure, Value};

    let argument = Value::Structure(Structure {
        id: geometry::ids::say::FUNCTION,
        fields: vec![StructureField {
            id: geometry::ids::say::TEXT,
            value: Box::new(Value::String("hello".into())),
        }],
    });
    let input = arora_buffers::serde_uuid::serialize(&argument);

    let output_address =
        geometry::__arora_export_say::arora_function_11112222_3333_4444_8555_000000000010(
            input.as_ptr() as usize,
        );

    let output = unsafe {
        let size = u32::from_le_bytes(*(output_address as *const [u8; 4])) as usize;
        std::slice::from_raw_parts(output_address as *const u8, size)
    };
    let Value::Structure(result) = arora_buffers::serde_uuid::deserialize(output) else {
        panic!("the result buffer is a structure");
    };
    assert_eq!(result.id, geometry::ids::say::FUNCTION);
    assert_eq!(
        Status::try_from((*result.fields[0].value).clone()).expect("a Status"),
        Status::Success,
        "the first field carries the return value"
    );
}
