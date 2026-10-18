//! The declared wasm guest and a host registration answer the same Calls.
use arora_animation_module::{
    animation::{self, ids},
    exports, AnimationModule, Clip, ClipKeypoint, ClipTrack, ClipTransition, PlayerState,
    TrackOutput,
};
use arora_engine::{
    engine::{EngineBuilder, PinnedEngine},
    executor::wasm::WebAssemblyExecutor,
    load::load_module_from_parts,
    module::HostModule,
};
use arora_types::{
    call::{Call, CallBridge},
    module::low::Executor,
    value::{StructureField, Value},
    Uuid,
};

fn call(engine: &mut PinnedEngine, id: Uuid, args: Vec<(Uuid, Value)>) -> Value {
    engine
        .arora_call(Call {
            module_id: Some(ids::MODULE),
            id,
            args: args
                .into_iter()
                .map(|(id, value)| StructureField {
                    id,
                    value: Box::new(value),
                })
                .collect(),
        })
        .expect("animation call")
        .ret
}
fn clip() -> Clip {
    let linear = || {
        vec![ClipTransition {
            kind: "linear".into(),
            dt_ns: 0.0,
            dv: 0.0,
        }]
    };
    Clip {
        name: "ramp".into(),
        length_ns: Some(1_000_000_000),
        tracks: vec![ClipTrack {
            id: "x-track".into(),
            name: "x".into(),
            target: "joint/x".into(),
            keypoints: [0, 1_000_000_000]
                .into_iter()
                .enumerate()
                .map(|(i, t)| ClipKeypoint {
                    id: i.to_string(),
                    time_ns: t,
                    value: Value::F64(i as f64),
                    arrive: linear(),
                    leave: linear(),
                    pairing: "free".into(),
                })
                .collect(),
        }],
    }
}

#[test]
fn wasm_and_host_linked_animation_match_through_calls() {
    let mut guest = EngineBuilder::new()
        .add_executor(WebAssemblyExecutor::new().unwrap())
        .build();
    let header = animation::header(Executor {
        name: "wasm".into(),
        min_version: None,
        max_version: None,
    });
    let bytes = std::fs::read(env!(
        "CARGO_CDYLIB_FILE_ARORA_ANIMATION_MODULE_arora_animation_module"
    ))
    .unwrap();
    load_module_from_parts(&mut guest, header, bytes.into_boxed_slice()).unwrap();
    let mut host = EngineBuilder::new().build();
    let module = HostModule::from_exports(ids::MODULE, exports(AnimationModule::new()));
    host.register_module(module.id(), Box::new(module));

    let mut both = |id, args: Vec<(Uuid, Value)>| {
        let expected = call(&mut host, id, args.clone());
        let actual = call(&mut guest, id, args);
        assert_eq!(actual, expected);
        actual
    };
    let anim = both(
        ids::load_clip::FUNCTION,
        vec![(ids::load_clip::CLIP, clip().into())],
    );
    assert_ne!(anim, Value::U32(u32::MAX));
    let player = both(
        ids::create_player::FUNCTION,
        vec![(ids::create_player::NAME, "ramp".into())],
    );
    let instance = both(
        ids::add_instance::FUNCTION,
        vec![
            (ids::add_instance::ANIM, anim),
            (ids::add_instance::PLAYER, player.clone()),
        ],
    );
    assert_ne!(instance, Value::U32(u32::MAX));
    assert_eq!(
        both(
            ids::play::FUNCTION,
            vec![(ids::play::PLAYER, player.clone())]
        ),
        player
    );
    for now in [
        0,
        125_000_000,
        250_000_000,
        500_000_000,
        875_000_000,
        1_125_000_000,
    ] {
        let outputs = both(
            ids::step::FUNCTION,
            vec![
                (ids::step::TIME_NS, Value::U64(now)),
                (
                    ids::step::DT_NS,
                    Value::U64(if now == 0 { 0 } else { 125_000_000 }),
                ),
            ],
        )
        .into_elements()
        .unwrap();
        assert_eq!(outputs.len(), 1);
        let output = TrackOutput::try_from(outputs[0].clone()).unwrap();
        let Value::F64(v) = output.value else {
            panic!("format 2 numbers output F64")
        };
        assert!((v - (now % 1_000_000_000) as f64 / 1e9).abs() < 1e-10);
        assert_eq!(output.default_key, "joint/x");
        let states = both(ids::player_states::FUNCTION, vec![])
            .into_elements()
            .unwrap();
        assert_eq!(states.len(), 1);
        let state = PlayerState::try_from(states[0].clone()).unwrap();
        assert_eq!(state.name, "ramp");
        assert_eq!(state.state, "playing");
        assert_eq!(state.time_ns, now % 1_000_000_000);
        assert_eq!(state.instances[0].rate, 1.0);
    }
}
