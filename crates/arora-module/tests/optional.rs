//! An `Option<T>` parameter or return is optional everywhere the declaration
//! reaches: the header, the record, the dispatch and the client stub.

use arora_engine::engine::{EngineBuilder, PinnedEngine};
use arora_engine::module::HostModule;
use arora_module::AroraModule;
use arora_types::call::{Call, CallBridge};
use arora_types::module::low::{Executor, ExportSymbol, TypeRef};
use arora_types::record::module::frozen::ExportKind;
use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use arora_types::{AroraType, Uuid};

#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "0d1e2f30-4a5b-4c6d-8e7f-000000000001")]
pub struct Window {
    #[arora(id = "0d1e2f30-4a5b-4c6d-8e7f-000000000011")]
    start: f32,
    #[arora(id = "0d1e2f30-4a5b-4c6d-8e7f-000000000012")]
    end: f32,
}

#[arora_module::module(
    id = "0d1e2f30-4a5b-4c6d-8e7f-000000000100",
    name = "sampler",
    version = "0.1.0"
)]
pub mod sampler {
    use super::Window;

    /// Describes a sampling request; `None` when the window is empty.
    #[export(id = "0d1e2f30-4a5b-4c6d-8e7f-000000000110")]
    pub fn describe(
        #[param(id = "0d1e2f30-4a5b-4c6d-8e7f-000000000111")] anim: u32,
        #[param(id = "0d1e2f30-4a5b-4c6d-8e7f-000000000112")] frame_rate: Option<f32>,
        #[param(id = "0d1e2f30-4a5b-4c6d-8e7f-000000000113")] window: Option<Window>,
    ) -> Option<String> {
        let window = window.unwrap_or(Window {
            start: 0.0,
            end: 1.0,
        });
        (window.end > window.start).then(|| {
            format!(
                "{anim} at {} Hz over {}..{}",
                frame_rate.unwrap_or(60.0),
                window.start,
                window.end
            )
        })
    }
}

fn engine() -> PinnedEngine {
    let mut engine = EngineBuilder::new().build();
    let module = HostModule::of::<sampler::Module>();
    engine.register_module(module.id(), Box::new(module));
    engine
}

fn describe(engine: &mut PinnedEngine, args: Vec<StructureField>) -> Value {
    engine
        .arora_call(Call {
            module_id: Some(sampler::ids::MODULE),
            id: sampler::ids::describe::FUNCTION,
            args,
        })
        .expect("describe dispatches")
        .ret
}

fn field(id: Uuid, value: Value) -> StructureField {
    StructureField {
        id,
        value: Box::new(value),
    }
}

fn some_string(text: &str) -> Value {
    Value::Option(Some(Box::new(Value::String(text.into()))))
}

#[test]
fn the_header_declares_an_optional_by_its_element_id() {
    let header = sampler::header(Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    });
    let ExportSymbol::Function(describe) = &header.exports[0];
    let optional_of = |ty: &TypeRef| match ty {
        TypeRef::Option { id } => *id,
        other => panic!("an optional, got {other:?}"),
    };
    assert_eq!(
        optional_of(&describe.parameters[1].ty),
        *arora_types::ty::F32_ID
    );
    assert_eq!(
        optional_of(&describe.parameters[2].ty),
        Window::arora_type_id()
    );
    assert_eq!(optional_of(&describe.ret), *arora_types::ty::STRING_ID);
}

#[test]
fn the_record_carries_the_optional_form() {
    let record = sampler::Module::record(Uuid::nil());
    let ExportKind::Function(describe) = &record
        .exports
        .get(&sampler::ids::describe::FUNCTION)
        .expect("describe")
        .kind;
    assert_eq!(
        describe.parameters[&sampler::ids::describe::FRAME_RATE].ty,
        FrozenTy::FrozenOption(FrozenOption {
            element: Box::new(FrozenTy::from(PrimitiveKind::F32)),
        })
    );
    let window = describe.parameters[&sampler::ids::describe::WINDOW]
        .ty
        .as_option()
        .expect("an optional");
    let FrozenTy::FrozenScalar(scalar) = &*window.element else {
        panic!("a structure element, got {:?}", window.element);
    };
    assert_eq!(scalar.reference.id, Window::arora_type_id());
    assert_eq!(scalar.reference.version, Window::arora_type_version());
}

#[test]
fn an_absent_optional_argument_is_none() {
    let mut engine = engine();
    let ret = describe(
        &mut engine,
        vec![field(sampler::ids::describe::ANIM, Value::U32(3))],
    );
    assert_eq!(ret, some_string("3 at 60 Hz over 0..1"));
}

#[test]
fn an_optional_argument_arrives_bare_or_wrapped() {
    let mut engine = engine();
    let anim = || field(sampler::ids::describe::ANIM, Value::U32(3));
    let rate = |value| field(sampler::ids::describe::FRAME_RATE, value);
    assert_eq!(
        describe(&mut engine, vec![anim(), rate(Value::F32(30.0))]),
        some_string("3 at 30 Hz over 0..1"),
        "a present value is the element itself"
    );
    assert_eq!(
        describe(
            &mut engine,
            vec![
                anim(),
                rate(Value::Option(Some(Box::new(Value::F32(30.0)))))
            ]
        ),
        some_string("3 at 30 Hz over 0..1"),
        "or a present `Value::Option`"
    );
    assert_eq!(
        describe(&mut engine, vec![anim(), rate(Value::Option(None))]),
        some_string("3 at 60 Hz over 0..1"),
        "an explicit `None` is absent"
    );
}

#[test]
fn a_required_argument_stays_required() {
    let mut engine = engine();
    let error = engine
        .arora_call(Call {
            module_id: Some(sampler::ids::MODULE),
            id: sampler::ids::describe::FUNCTION,
            args: vec![],
        })
        .expect_err("describe without its anim");
    assert!(
        error
            .to_string()
            .contains("missing parameter `anim` of `describe`"),
        "{error}"
    );
}

#[test]
fn an_empty_optional_return_is_none() {
    let mut engine = engine();
    let ret = describe(
        &mut engine,
        vec![
            field(sampler::ids::describe::ANIM, Value::U32(3)),
            field(
                sampler::ids::describe::WINDOW,
                Value::from(Window {
                    start: 1.0,
                    end: 1.0,
                }),
            ),
        ],
    );
    assert_eq!(ret, Value::Option(None));
}

#[test]
fn the_client_stub_takes_and_returns_options() {
    let mut engine = engine();
    assert_eq!(
        sampler::client::describe(&mut engine, 7, Some(24.0), None).expect("the stub calls"),
        Some("7 at 24 Hz over 0..1".to_string())
    );
    let empty = Window {
        start: 2.0,
        end: 1.0,
    };
    assert_eq!(
        sampler::client::describe(&mut engine, 7, None, Some(empty)).expect("the stub calls"),
        None
    );
}
