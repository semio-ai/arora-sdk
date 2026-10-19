//! Pins the old literal ABI and holds the host and guest declarations equal.
use arora_animation_module::{animation, animation_contract, exports, AnimationModule};
use arora_engine::{engine::EngineBuilder, module::HostModule};
use arora_types::{
    call::{Call, CallBridge},
    record::module::frozen::ExportKind,
    value::{StructureField, Value},
    Uuid,
};

#[test]
fn declarations_match_and_all_kept_function_and_parameter_ids_are_pinned() {
    let descriptions = animation_contract::descriptions();
    assert_eq!(descriptions.len(), 31);
    assert_eq!(animation::record(Uuid::nil()).exports, descriptions);
    assert_eq!(
        animation::ids::MODULE.to_string(),
        "76697a69-6a00-0000-0d00-000000000000"
    );
    let expected: serde_json::Value = serde_json::from_str(include_str!("kept_ids.json")).unwrap();
    assert_eq!(expected.as_array().unwrap().len(), 25);
    for function in expected.as_array().unwrap() {
        let id: Uuid = function["id"].as_str().unwrap().parse().unwrap();
        let actual = &descriptions[&id];
        assert_eq!(actual.name, function["name"].as_str().unwrap());
        let ExportKind::Function(signature) = &actual.kind;
        let parameters = function["parameters"].as_array().unwrap();
        assert_eq!(signature.parameters.len(), parameters.len());
        for (i, parameter) in parameters.iter().enumerate() {
            let id: Uuid = parameter["id"].as_str().unwrap().parse().unwrap();
            assert_eq!(signature.parameter_ordering[i], id);
            assert_eq!(
                signature.parameters[&id].name,
                parameter["name"].as_str().unwrap()
            );
        }
    }
}

fn call(engine: &mut impl CallBridge, id: Uuid, args: Vec<(Uuid, Value)>) -> Value {
    engine
        .arora_call(Call {
            module_id: Some(animation::ids::MODULE),
            id,
            args: args
                .into_iter()
                .map(|(id, value)| StructureField {
                    id,
                    value: Box::new(value),
                })
                .collect(),
        })
        .unwrap()
        .ret
}

#[test]
fn host_dispatch_matches_arguments_by_id_and_registrations_own_their_state() {
    let make = || {
        let mut engine = EngineBuilder::new().build();
        let module =
            HostModule::from_exports(animation::ids::MODULE, exports(AnimationModule::new()));
        assert_eq!(module.descriptions().len(), 31);
        engine.register_module(module.id(), Box::new(module));
        engine
    };
    let mut a = make();
    let mut b = make();
    let player = call(
        &mut a,
        animation::ids::create_player::FUNCTION,
        vec![(
            animation::ids::create_player::NAME,
            Value::String("a".into()),
        )],
    );
    // Intentionally reverse the declaration's parameter order.
    assert_eq!(
        call(
            &mut a,
            animation::ids::set_speed::FUNCTION,
            vec![
                (animation::ids::set_speed::SPEED, Value::F32(-2.0)),
                (animation::ids::set_speed::PLAYER, player.clone())
            ]
        ),
        player
    );
    let states = call(&mut a, animation::ids::player_states::FUNCTION, vec![])
        .into_elements()
        .unwrap();
    let state = arora_animation_module::PlayerState::try_from(states[0].clone()).unwrap();
    // Establish the device clock to apply commands issued before the first step.
    call(
        &mut a,
        animation::ids::step::FUNCTION,
        vec![(animation::ids::step::DT_NS, Value::U64(0))],
    );
    let states = call(&mut a, animation::ids::player_states::FUNCTION, vec![])
        .into_elements()
        .unwrap();
    assert_eq!(
        arora_animation_module::PlayerState::try_from(states[0].clone())
            .unwrap()
            .speed,
        -2.0
    );
    assert_eq!(state.name, "a");
    assert!(
        call(&mut b, animation::ids::player_states::FUNCTION, vec![])
            .into_elements()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        call(
            &mut b,
            animation::ids::pause::FUNCTION,
            vec![(animation::ids::pause::PLAYER, player)]
        ),
        Value::U32(u32::MAX)
    );
}

/// The module is `@arora/animation`: named `animation`, without the
/// `-module` its crate carries, in the `arora` scope, at 3.0.0.
#[test]
fn the_module_is_animation_in_the_arora_scope() {
    use arora_types::module::declared::AroraModule;
    use arora_types::module::low::Executor;
    let header = animation::header(Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    });
    assert_eq!(header.name, "animation");
    assert_eq!(header.version.to_string(), "3.0.0");
    assert_eq!(animation::record(Uuid::nil()).name, "animation");
    assert_eq!(
        animation::Module::scope(),
        Some(arora_types::record::scope::ARORA)
    );
}

/// Each record's version. A record `@vizij/animation` 2.0.0 publishes is at
/// the next minor above its version there, so the two modules' records,
/// which differ by their scope, load into one registry.
#[test]
fn records_are_at_their_versions() {
    use arora_animation_module::*;
    use arora_types::AroraType;
    fn version<T: AroraType>() -> String {
        T::arora_type_version().to_string()
    }
    for (record, actual, expected) in [
        ("AnimationClip", version::<AnimationClip>(), "1.2.0"),
        ("AnimTrack", version::<AnimTrack>(), "1.2.0"),
        ("Keypoint", version::<Keypoint>(), "1.2.0"),
        ("TransitionHandle", version::<TransitionHandle>(), "1.1.0"),
        ("TrackOutput", version::<TrackOutput>(), "1.1.0"),
        ("PlayerState", version::<PlayerState>(), "1.3.0"),
        ("InstanceState", version::<InstanceState>(), "1.2.0"),
        ("StepValues", version::<StepValues>(), "1.1.0"),
        ("OutputKeys", version::<OutputKeys>(), "1.1.0"),
        ("StepSamples", version::<StepSamples>(), "1.0.0"),
        ("BakedAnimation", version::<BakedAnimation>(), "1.1.0"),
        ("BakedTrack", version::<BakedTrack>(), "1.1.0"),
        ("Clip", version::<Clip>(), "2.0.0"),
        ("ClipTrack", version::<ClipTrack>(), "2.0.0"),
        ("ClipKeypoint", version::<ClipKeypoint>(), "2.0.0"),
        ("ClipTransition", version::<ClipTransition>(), "2.0.0"),
    ] {
        assert_eq!(actual, expected, "{record}");
    }
}
