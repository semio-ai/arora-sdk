//! Boundary conversion and module behavior beyond the animation library's own tests.
use arora_animation::{clip as core, clip1::ClipV1, Curve};
use arora_animation_module::*;
use arora_types::{value::Value, AroraType};
const SECOND: u64 = 1_000_000_000;

fn point(t: i64, value: Value) -> ClipKeypoint {
    ClipKeypoint {
        id: t.to_string(),
        time_ns: t,
        value,
        arrive: vec![ClipTransition {
            kind: "linear".into(),
            dt_ns: 0.0,
            dv: 0.0,
        }],
        leave: vec![ClipTransition {
            kind: "linear".into(),
            dt_ns: 0.0,
            dv: 0.0,
        }],
        pairing: "".into(),
    }
}
fn track(id: &str, values: Vec<ClipKeypoint>) -> ClipTrack {
    ClipTrack {
        id: id.into(),
        name: id.into(),
        target: format!("joint/{id}"),
        keypoints: values,
    }
}
fn clip() -> Clip {
    Clip {
        name: "ramp".into(),
        length_ns: Some(SECOND as i64),
        tracks: vec![track(
            "x",
            vec![
                point(0, Value::F64(0.0)),
                point(SECOND as i64, Value::F64(1.0)),
            ],
        )],
    }
}
fn setup(clip: Clip) -> (AnimationModule, u32, u32, u32) {
    let mut a = AnimationModule::new();
    let anim = a.load_clip(clip);
    assert_ne!(anim, u32::MAX);
    let p = a.create_player(Some("player".into()));
    let i = a.add_instance(p, anim);
    (a, anim, p, i)
}
fn array(v: Value) -> Vec<Value> {
    let Value::ArrayValue(v) = v else {
        panic!("expected dynamic array")
    };
    v
}
fn near(v: &Value, expected: f64) {
    let n = match v {
        Value::F64(v) => *v,
        Value::F32(v) => f64::from(*v),
        _ => panic!("expected number: {v:?}"),
    };
    assert!((n - expected).abs() < 1e-6, "{n} != {expected}");
}

#[test]
fn format_two_curve_values_and_boolean_text_step_rule() {
    let mut c = clip();
    c.tracks[0].keypoints[0].leave[0].kind = "sine".into();
    c.tracks.push(track(
        "bool",
        vec![
            point(0, false.into()),
            point(500_000_000, true.into()),
            point(SECOND as i64, false.into()),
        ],
    ));
    c.tracks.push(track(
        "text",
        vec![
            point(0, "a".into()),
            point(500_000_000, "b".into()),
            point(SECOND as i64, "c".into()),
        ],
    ));
    let core = core::Clip::try_from(c.clone()).unwrap();
    let curve = Curve::new(&core.tracks[0]);
    let (mut a, _, p, _) = setup(c);
    a.set_loop(p, "once".into());
    for t in [0, 250_000_000, 500_000_000, 500_000_001, SECOND] {
        let out = a.step(t, Some(t));
        let arora_animation::Value::Number(n) = curve.sample(t as f64).unwrap().value else {
            panic!()
        };
        near(&out[0].value, n);
        assert_eq!(
            (&out[0].track_id, &out[0].default_key),
            (&"x".into(), &"joint/x".into())
        );
        let interior_after = t > 500_000_000 && t < SECOND;
        assert_eq!(out[1].value, Value::Boolean(interior_after));
        assert_eq!(
            out[2].value,
            Value::String(
                if t == SECOND {
                    "c"
                } else if interior_after {
                    "b"
                } else {
                    "a"
                }
                .into()
            )
        );
    }
    let velocities = array(a.step_with_velocities(0, None).velocities);
    assert_eq!(velocities[1..], [Value::Unit, Value::Unit]);
}

#[test]
fn signed_keypoints_offsets_and_rate_scale_time_and_velocity() {
    let mut c = clip();
    c.tracks[0].keypoints = vec![
        point(-(SECOND as i64), Value::F64(-1.0)),
        point(SECOND as i64, Value::F64(1.0)),
    ];
    let (mut a, _, p, i) = setup(c);
    a.set_start_offset(p, i, -250_000_000);
    a.set_rate(p, i, 2.0);
    let samples = a.step_with_velocities(0, Some(0));
    near(&array(samples.values)[0], 0.5);
    near(&array(samples.velocities)[0], 2.0);
    // A positive timeline offset reaches a negative clip-local keypoint.
    a.set_start_offset(p, i, 500_000_000);
    near(&a.step(0, None)[0].value, -1.0);
    let state = &a.player_states()[0];
    assert_eq!(state.instances[0].rate, 2.0);
    assert_eq!(state.instances[0].time_scale, 0.5);
    for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(a.set_rate(p, i, rate), u32::MAX);
    }
    for ts in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(a.set_time_scale(p, i, ts), u32::MAX);
    }
    let (mut a, _, p, i) = setup(clip());
    let (mut b, _, q, j) = setup(clip());
    a.set_time_scale(p, i, 2.0);
    b.set_rate(q, j, 0.5);
    assert_eq!(
        a.step_with_velocities(SECOND / 2, None),
        b.step_with_velocities(SECOND / 2, None)
    );
}

fn legacy() -> AnimationClip {
    AnimationClip {
        name: "old".into(),
        duration: 1000,
        tracks: vec![AnimTrack {
            id: "narrow".into(),
            name: "".into(),
            animatable_id: "narrow".into(),
            points: vec![
                Keypoint {
                    id: "a".into(),
                    stamp: 0.0,
                    value: Value::F32(0.0),
                    transitions_in: vec![],
                    transitions_out: vec![],
                },
                Keypoint {
                    id: "b".into(),
                    stamp: 1.0,
                    value: Value::F32(1.0),
                    transitions_in: vec![],
                    transitions_out: vec![],
                },
            ],
        }],
    }
}
#[test]
fn legacy_conversion_preserves_curve_and_number_width_and_refuses_composites() {
    let mut c = legacy();
    let mut wide = c.tracks[0].clone();
    wide.id = "wide".into();
    wide.animatable_id = "wide".into();
    wide.points[1].value = Value::F64(1.0);
    c.tracks.push(wide);
    let mut integer = c.tracks[1].clone();
    integer.id = "int".into();
    integer.animatable_id = "int".into();
    integer.points[1].value = Value::I64(1);
    c.tracks.push(integer);
    let (core, narrow) = ClipV1::try_from(c.clone()).unwrap().to_clip().unwrap();
    assert_eq!(narrow, [true, false, false]);
    let mut a = AnimationModule::new();
    let anim = a.load_animation(c.clone());
    let p = a.create_player(None);
    a.add_instance(p, anim);
    for t in [0, 125_000_000, 500_000_000, 875_000_000] {
        let values = array(a.step_values(t, Some(t)).values);
        assert!(
            matches!(values[0], Value::F32(_))
                && matches!(values[1], Value::F64(_))
                && matches!(values[2], Value::F64(_))
        );
        let arora_animation::Value::Number(n) =
            Curve::new(&core.tracks[0]).sample(t as f64).unwrap().value
        else {
            panic!()
        };
        near(&values[0], n);
        near(&values[1], n);
    }
    let bake = a
        .bake_with_derivatives(anim, Some(4.0), None, None)
        .unwrap();
    assert!(array(bake.tracks[0].values.clone())
        .iter()
        .all(|v| matches!(v, Value::F32(_))));
    assert!(array(bake.tracks[1].values.clone())
        .iter()
        .all(|v| matches!(v, Value::F64(_))));
    assert!(array(bake.tracks[0].derivatives.clone())
        .iter()
        .all(|v| matches!(v, Value::Option(Some(x)) if matches!(**x, Value::F64(_)))));
    c.tracks[0].points[0].value = Value::ArrayF64(vec![1.0]);
    assert_eq!(a.load_animation(c.clone()), u32::MAX);
    assert!(!a.reload_animation(anim, c));
}

#[test]
fn clip_records_round_trip_and_invalid_clips_leave_loaded_state_intact() {
    let mut c = clip();
    c.tracks[0].keypoints[0].pairing = "paired".into();
    c.tracks[0].keypoints[0].leave = vec![ClipTransition {
        kind: "handle".into(),
        dt_ns: 200_000_000.0,
        dv: 0.25,
    }];
    c.tracks[0].keypoints[1].arrive[0].kind = "inferred-auto-clamped".into();
    let core = core::Clip::try_from(c.clone()).unwrap();
    assert_eq!(Clip::from(core), c);
    assert_eq!(Clip::try_from(Value::from(c.clone())).unwrap(), c);
    let (mut a, anim, _, _) = setup(c.clone());
    for bad in [
        "pairing",
        "transition",
        "side",
        "value",
        "mixed",
        "infinite",
    ] {
        let mut bad_clip = c.clone();
        let k = &mut bad_clip.tracks[0].keypoints[0];
        match bad {
            "pairing" => k.pairing = "unknown".into(),
            "transition" => k.leave[0].kind = "unknown".into(),
            "side" => k.leave.push(k.leave[0].clone()),
            "value" => k.value = Value::Unit,
            "mixed" => k.value = Value::Boolean(true),
            "infinite" => k.value = Value::F64(f64::INFINITY),
            _ => unreachable!(),
        }
        assert_eq!(a.load_clip(bad_clip.clone()), u32::MAX, "{bad}");
        assert!(!a.reload_clip(anim, bad_clip), "{bad}");
    }
    assert_eq!(a.step(0, None)[0].value, Value::F64(0.0));
    for transition in [
        "linear",
        "sine",
        "quad",
        "cubic",
        "quart",
        "quint",
        "expo",
        "circ",
        "back",
        "explicit-handles",
        "inferred-auto-clamped",
    ] {
        let t = ClipTransition {
            kind: transition.into(),
            dt_ns: 0.0,
            dv: 0.0,
        };
        assert_eq!(
            ClipTransition::from(core::Transition::try_from(t.clone()).unwrap()),
            t
        );
    }
    assert_eq!(PlayerState::arora_type_version().to_string(), "1.3.0");
    assert_eq!(InstanceState::arora_type_version().to_string(), "1.2.0");
    assert_eq!(Clip::arora_type_version().to_string(), "2.0.0");
    let state = a.player_states().remove(0);
    assert_eq!(
        PlayerState::try_from(Value::from(state.clone())).unwrap(),
        state
    );
}

#[test]
fn transport_is_immediate_and_reports_windows_heading_end_and_instance_timing() {
    let (mut a, _, p, i) = setup(clip());
    a.step(0, Some(0));
    let state = &a.player_states()[0];
    assert_eq!(
        (
            &state.name,
            state.state.as_str(),
            state.speed,
            state.loop_mode.as_str()
        ),
        (&"player".into(), "playing", 1.0, "loop")
    );
    for mode in ["once", "loop", "bounce", "ping_pong"] {
        assert_eq!(a.set_loop(p, mode.into()), p);
        assert_eq!(a.player_states()[0].loop_mode, mode);
    }
    assert_eq!(a.set_loop(p, "pingpong".into()), u32::MAX);
    a.seek(p, 100_000_000);
    a.set_window(p, 250_000_000, Some(750_000_000));
    let state = &a.player_states()[0];
    assert_eq!(
        state.time_ns, 100_000_000,
        "setting the window does not move the playhead"
    );
    assert_eq!(
        (state.window_start_ns, state.window_end_ns),
        (250_000_000, Some(750_000_000))
    );
    a.set_loop(p, "bounce".into());
    a.play(p);
    a.step(800_000_000, Some(800_000_000));
    let state = &a.player_states()[0];
    assert_eq!(state.time_ns, 600_000_000);
    assert!(state.reversed);
    assert!(!state.ended);
    a.step(400_000_000, None);
    let state = &a.player_states()[0];
    assert_eq!(state.time_ns, 250_000_000);
    assert!(state.ended);
    assert!(state.reversed);
    // Given the heading of a player that turned, a bounce turns no more.
    let mut b = AnimationModule::new();
    b.step(0, Some(0));
    let q = b.create_player(Some("q".into()));
    b.set_length(q, Some(SECOND));
    b.set_loop(q, "bounce".into());
    b.seek(q, 600_000_000);
    b.play(q);
    assert_eq!(b.set_reversed(q, true), q);
    assert!(b.player_states()[0].reversed);
    b.step(SECOND, None);
    let late = &b.player_states()[0];
    assert_eq!(late.time_ns, 0);
    assert!(late.ended);
    assert_eq!(b.set_reversed(q + 1, true), u32::MAX);
    a.pause(p);
    assert_eq!(a.player_states()[0].state, "paused");
    a.stop(p);
    assert_eq!(a.player_states()[0].state, "stopped");
    a.seek(p, 600_000_000);
    a.set_speed(p, -1.0);
    a.set_loop(p, "once".into());
    a.play(p);
    a.step(100_000_000, None);
    assert_eq!(a.player_states()[0].time_ns, 500_000_000);
    assert!(!a.player_states()[0].reversed);
    a.pause(p);
    a.seek(p, 300_000_000);
    a.set_speed(p, 1.0);
    let now = 1_300_000_000;
    a.play_at(p, now + 200_000_000);
    a.step(100_000_000, None);
    assert_eq!(a.player_states()[0].time_ns, 300_000_000);
    assert_eq!(a.player_states()[0].state, "playing");
    assert!(a.player_states()[0].waiting, "held until its start");
    a.step(200_000_000, None);
    assert_eq!(a.player_states()[0].time_ns, 400_000_000);
    assert!(!a.player_states()[0].waiting);
    a.set_rate(p, i, 0.5);
    assert_eq!(a.player_states()[0].duration_ns, 2 * SECOND);
    assert_eq!(a.player_states()[0].instances[0].rate, 0.5);
    for speed in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(a.set_speed(p, speed), u32::MAX);
    }
    for weight in [f32::NAN, f32::INFINITY, -1.0] {
        assert_eq!(a.set_weight(p, i, weight), u32::MAX);
        assert_eq!(a.add_instance_with_weight(p, 0, weight), u32::MAX);
    }
    for reply in [
        a.play(u32::MAX),
        a.pause(u32::MAX),
        a.stop(u32::MAX),
        a.seek(u32::MAX, 0),
        a.play_at(u32::MAX, 0),
        a.set_speed(u32::MAX, 1.0),
        a.set_loop(u32::MAX, "once".into()),
        a.set_window(u32::MAX, 0, None),
        a.set_rate(p, u32::MAX, 1.0),
    ] {
        assert_eq!(reply, u32::MAX);
    }
}

#[test]
fn output_revisions_silence_reload_and_removal_follow_the_engine_table() {
    let (mut a, anim, p, i) = setup(clip());
    let keys = a.output_keys();
    assert_eq!(keys.keys, ["joint/x"]);
    a.set_weight(p, i, 0.0);
    let samples = a.step_with_velocities(0, None);
    assert_eq!(samples.revision, keys.revision);
    assert_eq!(array(samples.values), [Value::Unit]);
    assert_eq!(array(samples.velocities), [Value::Unit]);
    assert!(a.step(0, None).is_empty());
    a.set_weight(p, i, 1.0);
    a.pause(p);
    let samples = a.step_values(0, None);
    assert_eq!(samples.revision, keys.revision);
    let mut c = clip();
    c.tracks[0].id = "replacement".into();
    assert!(a.reload_clip(anim, c.clone()));
    assert_ne!(a.output_keys().revision, keys.revision);
    assert_eq!(a.step(0, None)[0].track_id, "replacement");
    c.tracks[0].target = "new".into();
    assert!(a.reload_clip(anim, c));
    assert_ne!(a.output_keys().revision, keys.revision);
    assert_eq!(a.output_keys().keys, ["new"]);
    let q = a.create_player(None);
    a.add_instance(q, anim);
    assert_eq!(a.output_keys().keys, ["new", "new"]);
    assert_eq!(a.remove_instance(p, i), 1);
    assert_eq!(a.remove_instance(p, i), 0);
    assert!(a.remove_player(p));
    assert!(!a.remove_player(p));
    assert!(a.unload_animation(anim));
    assert!(!a.unload_animation(anim));
    assert!(a.output_keys().keys.is_empty());
    assert!(a.player_states()[0].instances.is_empty());
    assert_eq!(a.add_instance(q, anim), u32::MAX);
}

#[test]
fn bakes_have_values_and_optional_f64_derivatives_in_clip_local_units() {
    let mut c = clip();
    c.tracks.push(track("text", vec![point(0, "hi".into())]));
    c.tracks.push(track("bool", vec![point(0, true.into())]));
    c.tracks.push(track("empty", vec![]));
    let (mut a, anim, _, _) = setup(c);
    let plain = a
        .bake(anim, Some(4.0), Some(250_000_000), Some(750_000_000))
        .unwrap();
    let baked = a
        .bake_with_derivatives(anim, Some(4.0), Some(250_000_000), Some(750_000_000))
        .unwrap();
    assert_eq!(
        (baked.frame_rate, baked.start_ns, baked.end_ns),
        (4.0, 250_000_000, 750_000_000)
    );
    assert_eq!(baked.tracks.len(), 4);
    assert_eq!(baked.tracks[0].animatable_id, "joint/x");
    for (sample, expected) in array(baked.tracks[0].values.clone())
        .iter()
        .zip([0.25, 0.5, 0.75])
    {
        near(sample, expected);
    }
    for (p, b) in plain.tracks.iter().zip(&baked.tracks) {
        assert_eq!(p.values, b.values);
        assert!(array(p.derivatives.clone()).is_empty());
    }
    for d in array(baked.tracks[0].derivatives.clone()) {
        let Value::Option(Some(d)) = d else { panic!() };
        near(&d, 1.0);
    }
    assert_eq!(
        array(baked.tracks[1].derivatives.clone()),
        vec![Value::Option(None); 3]
    );
    assert_eq!(
        array(baked.tracks[2].derivatives.clone()),
        vec![Value::Option(None); 3]
    );
    assert!(array(baked.tracks[3].values.clone()).is_empty());
    assert!(array(baked.tracks[3].derivatives.clone()).is_empty());
    assert_eq!(
        BakedAnimation::try_from(Value::from(baked.clone())).unwrap(),
        baked
    );
    assert_eq!(a.bake(anim, None, None, None).unwrap().frame_rate, 60.0);
    assert!(a.bake(u32::MAX, None, None, None).is_none());
    for rate in [0.0, f32::NAN, 1e9] {
        assert!(a.bake(anim, Some(rate), None, None).is_none());
    }
}

/// Ports weighted blending, speed retention and reload preservation from the old wrapper.
#[test]
fn weights_blend_widths_and_reload_keeps_transport_and_instance_settings() {
    let mut a = AnimationModule::new();
    let mut narrow = legacy();
    narrow.tracks[0].points.truncate(1);
    narrow.tracks[0].points[0].value = Value::F32(0.25);
    let first = a.load_animation(narrow.clone());
    let mut wide = narrow.clone();
    wide.tracks[0].points[0].value = Value::F64(0.75);
    let second = a.load_animation(wide);
    let p = a.create_player(None);
    let i = a.add_instance(p, first);
    let j = a.add_instance_with_weight(p, second, 3.0);
    let blended = &a.step(100_000_000, None)[0].value;
    assert!(matches!(blended, Value::F64(_)));
    near(blended, 0.625);
    a.set_weight(p, j, 0.0);
    assert_eq!(a.step(0, None)[0].value, Value::F32(0.25));
    a.set_speed(p, 0.5);
    a.pause(p);
    a.step(100_000_000, None);
    assert_eq!(a.player_states()[0].time_ns, 100_000_000);
    a.play(p);
    a.step(100_000_000, None);
    let before = a.player_states().remove(0);
    assert_eq!(before.time_ns, 150_000_000);
    assert_eq!(before.speed, 0.5);
    narrow.duration = 2000;
    narrow.tracks[0].points[0].value = Value::F32(0.5);
    assert!(a.reload_animation(first, narrow));
    let after = a.player_states().remove(0);
    assert_eq!(after.time_ns, before.time_ns);
    assert_eq!(after.speed, before.speed);
    assert_eq!(after.instances, before.instances);
    assert_eq!(after.duration_ns, 2 * SECOND);
    assert_eq!(a.step(0, None)[0].value, Value::F32(0.5));
    assert_eq!(a.remove_instance(p, i), 1);
    assert!(
        a.step(0, None).is_empty(),
        "remaining instance has weight zero"
    );
}
