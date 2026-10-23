//! The declared wasm guest and a host registration answer the same Calls, and
//! a device runner given the guest's module directory describes every export.
use arora_animation_module::{
    animation::{self, ids},
    exports, AnimationModule, Clip, ClipKeypoint, ClipTrack, ClipTransition, PlayerState,
    TrackOutput,
};
use arora_engine::{
    engine::{EngineBuilder, PinnedEngine},
    executor::component::ComponentExecutor,
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
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let header = animation::header(Executor {
        name: "wasm-component".into(),
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

/// A device runner given the module directory the guest ships in
/// (`--module`) loads it, and describes every export from the module's
/// record: the record-typed ones a stepping behavior calls (`step_values`,
/// `player_states`) and the 2.0 clip loader with their record types, at the
/// versions the declaration gives them.
#[test]
fn a_runner_given_the_guest_s_module_directory_describes_its_record_typed_exports() {
    use arora_animation_module::{module_dir, StepValues};
    use arora_types::record::reference::FrozenReference;
    use arora_types::record::ty::{FrozenScalar, FrozenTy};
    use arora_types::AroraType;
    use std::future::Future;
    use std::task::{Context, Poll};

    let root = std::env::temp_dir().join(format!(
        "arora-animation-module-directory-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let device_dir = root.join("device");
    let dir = root.join("animation");
    let guest = std::path::PathBuf::from(env!(
        "CARGO_CDYLIB_FILE_ARORA_ANIMATION_MODULE_arora_animation_module"
    ));
    module_dir::write(&dir, Some(&guest)).expect("the module directory is written");

    let mut device = futures::executor::block_on(
        arora::Arora::builder().with_module_dirs(&device_dir, std::slice::from_ref(&dir)),
    )
    .expect("the module directory and its records load")
    .build()
    .expect("the record describes the header");

    let caller = device.caller();
    let mut methods = std::pin::pin!(caller.describe_methods(None));
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    let methods = (0..8)
        .find_map(|_| {
            device
                .step(std::time::Duration::from_millis(10))
                .expect("the device steps");
            match methods.as_mut().poll(&mut cx) {
                Poll::Ready(methods) => Some(methods.expect("the device describes its methods")),
                Poll::Pending => None,
            }
        })
        .expect("an answer within eight steps");
    for (name, function) in [
        ("step_values", ids::step_values::FUNCTION),
        ("player_states", ids::player_states::FUNCTION),
        ("output_keys", ids::output_keys::FUNCTION),
        ("load_clip", ids::load_clip::FUNCTION),
        ("set_reversed", ids::set_reversed::FUNCTION),
    ] {
        let method = methods
            .iter()
            .find(|method| method.name == name)
            .unwrap_or_else(|| panic!("{name} is described"));
        assert_eq!(method.id, function, "{name}");
    }
    let step_values = methods.iter().find(|m| m.name == "step_values").unwrap();
    assert_eq!(
        step_values.function.return_ty,
        FrozenTy::FrozenScalar(FrozenScalar {
            reference: FrozenReference {
                id: StepValues::arora_type_id(),
                version: StepValues::arora_type_version(),
            },
        })
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// The records `@vizij/animation-module` 4.1.0 publishes in the `vizij`
/// scope (`fixtures/vizij-animation-module-4.1.0`) and those of this
/// module's directory load into one registry, in either order, as a device
/// loads its module directories' records, and into a registry a host adds
/// the declared module to: no record of one holds an id and version a
/// different record of the other holds. A record both carry is held at both
/// versions, each in its own scope.
#[test]
fn vizij_s_published_records_and_the_module_directory_s_load_into_one_registry() {
    use arora::module_dir::{registry, ModuleFiles};
    use arora_animation_module::{module_dir, AnimationClip, OutputKeys, StepValues};
    use arora_registry::local::LocalRegistry;
    use arora_registry::local_yaml::load_records_from_yaml_dir;
    use arora_registry::TypeDefinitionFrozen;
    use arora_types::record::scope::ARORA;
    use arora_types::record::Version;
    use arora_types::AroraType;
    use futures::executor::block_on;
    use std::path::{Path, PathBuf};

    let vizij = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("vizij-animation-module-4.1.0");
    let arora = std::env::temp_dir().join("arora-animation-module-directory-beside-vizij");
    let _ = std::fs::remove_dir_all(&arora);
    module_dir::write(&arora, None).expect("the module directory is written");
    let files = |dir: &Path| ModuleFiles {
        dir: dir.to_path_buf(),
        header: serde_yaml::from_str(
            &std::fs::read_to_string(dir.join("header.json")).expect("the header reads"),
        )
        .expect("the header parses"),
        executable: Box::new([]),
    };

    let v = |s: &str| Version::parse(s).expect("a valid version").0;
    let parent = |registry: &LocalRegistry, id, version: &str| match registry
        .type_at(&id, &v(version))
        .unwrap_or_else(|| panic!("{id}@{version} is held"))
    {
        TypeDefinitionFrozen::Structure(structure) => structure.parent,
        other => panic!("{id}@{version} is a structure, not {other:?}"),
    };
    let vizij_scope = parent(
        &block_on(registry(&[files(&vizij)])).expect("vizij's records load"),
        AnimationClip::arora_type_id(),
        "1.1.0",
    );
    assert_ne!(vizij_scope, ARORA.id);
    let holds_both = |registry: &LocalRegistry| {
        for (id, vizij_version, version) in [
            (AnimationClip::arora_type_id(), "1.1.0", "1.2.0"),
            (StepValues::arora_type_id(), "1.0.0", "1.1.0"),
            (OutputKeys::arora_type_id(), "1.0.0", "1.1.0"),
        ] {
            assert_eq!(parent(registry, id, vizij_version), vizij_scope);
            assert_eq!(parent(registry, id, version), ARORA.id);
        }
        let module = animation::ids::MODULE;
        assert_eq!(
            registry.module_at(&module, &v("2.0.0")).unwrap().name,
            "animation"
        );
        assert_eq!(
            registry.module_at(&module, &v("3.0.0")).unwrap().name,
            "animation"
        );
    };

    for modules in [
        [files(&arora), files(&vizij)],
        [files(&vizij), files(&arora)],
    ] {
        let registry = block_on(registry(&modules)).expect("both modules' records load");
        holds_both(&registry);
    }
    let mut registry = LocalRegistry::new();
    block_on(load_records_from_yaml_dir(
        vizij.join("records"),
        &mut registry,
    ))
    .expect("vizij's records load");
    block_on(registry.add_declared_module::<animation::Module>(v("3.0.0")))
        .expect("the declared module adds beside them");
    holds_both(&registry);
    std::fs::remove_dir_all(&arora).unwrap();
}
