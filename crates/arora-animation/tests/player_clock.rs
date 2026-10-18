//! A player's clock: it stays on its device's timeline however long it runs
//! and however the device steps, a step samples what a bake takes at the
//! same time, and devices sharing a clock and a start play in lockstep.

use arora_animation::{Clip, Engine, LoopMode, Nanos, PlayerId, Timing, Value};

const MS: Nanos = 1_000_000;
const SECOND: Nanos = 1_000 * MS;

/// A clip `ms` long, one track from -100 to 100 on a cubic ease.
fn ramp(ms: i64) -> Clip {
    Clip::from_json(&format!(
        r#"{{"lengthNs":{},"tracks":[{{"id":"t","animatableId":"x","points":[
            {{"id":"a","timeNs":0,"value":-100}},
            {{"id":"b","timeNs":{},"value":100}}]}}]}}"#,
        ms * MS,
        ms * MS
    ))
    .unwrap()
}

/// An engine playing `clip` on one player in `mode` at `speed`.
fn playing(clip: Clip, mode: LoopMode, speed: f64) -> (Engine, PlayerId) {
    let mut engine = Engine::new();
    let anim = engine.load(clip, vec![]);
    let player = engine.create_player("p");
    engine
        .add_instance(player, anim, 1.0, Timing::default())
        .unwrap();
    engine.set_mode(player, mode);
    engine.set_speed(player, speed);
    engine.play(player);
    (engine, player)
}

#[test]
fn a_player_stepped_at_100_hz_for_an_hour_stays_on_its_timeline() {
    // A third: each step's travel is a fraction of a nanosecond off a whole
    // one, so a clock summing its steps would drift.
    let speed = 1.0 / 3.0;
    let (mut engine, _) = playing(ramp(7_200_000), LoopMode::Once, speed);
    let dt = 10 * MS;
    for n in 0..=360_000 {
        engine.step(n * dt, if n == 0 { 0 } else { dt });
        let expected = (speed * (n * dt) as f64).round() as Nanos;
        assert_eq!(engine.players()[0].position, expected, "step {n}");
    }
}

#[test]
fn a_once_player_ends_on_the_step_that_reaches_its_end() {
    for (ms, dt) in [
        (5_000, 10 * MS),
        (1_000, 100 * MS),
        (10_000, 20 * MS),
        (1_000, 16_666_667),
    ] {
        let (mut engine, _) = playing(ramp(ms), LoopMode::Once, 1.0);
        let end = ms * MS;
        let mut t = 0;
        engine.step(0, 0);
        while t + dt < end {
            t += dt;
            engine.step(t, dt);
            assert!(
                !engine.players()[0].ended,
                "{ms} ms at {dt} ns: ended at {t}"
            );
        }
        engine.step(t + dt, dt);
        let player = &engine.players()[0];
        assert!(player.ended, "{ms} ms at {dt} ns");
        assert_eq!(player.position, end);
    }
}

#[test]
fn a_step_samples_the_frame_a_bake_takes_at_its_time() {
    let (mut engine, _) = playing(ramp(60_000), LoopMode::Once, 1.0);
    let anim = arora_animation::AnimId(0);
    let bake = engine.bake(anim, 100.0, None, None).unwrap();
    let frames = &bake.tracks[0].1;
    assert_eq!(frames.len(), 6_001);
    let dt = 10 * MS;
    for (n, frame) in frames.iter().enumerate() {
        let n = n as Nanos;
        let out = engine.step(n * dt, if n == 0 { 0 } else { dt });
        assert_eq!(out[0].sample.as_ref().unwrap(), frame, "frame {n}");
    }
}

#[test]
fn a_looping_player_runs_for_a_month_without_drifting() {
    // A 1.37 s idle loop, stepped once an hour for thirty days, stands where
    // the loop's period puts it, as one stepped every millisecond does.
    let length = 1_370 * MS;
    let (mut hourly, _) = playing(ramp(1_370), LoopMode::Loop, 1.0);
    hourly.step(0, 0);
    let hour = 3_600 * SECOND;
    for h in 1..=720 {
        hourly.step(h * hour, hour);
        let expected = (h * hour) % length;
        let position = hourly.players()[0].position;
        assert!(
            position == expected || (expected == 0 && position == length),
            "hour {h}: {position}"
        );
    }
    let month = 720 * hour;
    let (mut fine, _) = playing(ramp(1_370), LoopMode::Loop, 1.0);
    fine.step(month - SECOND, month - SECOND);
    for k in 1..=1_000 {
        fine.step(month - SECOND + k * MS, MS);
    }
    assert_eq!(fine.players()[0].position, hourly.players()[0].position);
    assert_eq!(fine.outputs(), hourly.outputs());
}

#[test]
fn devices_sharing_a_clock_and_a_start_play_in_lockstep() {
    // An emulated device stepping with the page, every 16 or 17 ms, and a
    // robot at 1 kHz, told to start at the same device time and to bounce
    // and loop: wherever their steps meet, they write the same values.
    let start = 5 * SECOND + 123;
    let make = || {
        let (mut engine, player) = playing(ramp(2_000), LoopMode::PingPong, 1.0);
        engine.set_window(player, 250 * MS, Some(1_750 * MS));
        engine.set_speed(player, 1.5);
        engine.play_at(player, start);
        engine
    };
    let (mut page, mut robot) = (make(), make());
    page.step(0, 0);
    robot.step(0, 0);
    let (mut page_t, mut robot_t) = (0, 0);
    let mut met = 0;
    let mut frame = 0;
    while robot_t < 10 * 60 * SECOND {
        robot_t += MS;
        robot.step(robot_t, MS);
        let page_dt = if frame % 3 == 1 { 17 * MS } else { 16 * MS };
        if page_t + page_dt <= robot_t {
            page_t += page_dt;
            page.step(page_t, page_dt);
            frame += 1;
        }
        if page_t == robot_t {
            met += 1;
            assert_eq!(page.outputs(), robot.outputs(), "at {robot_t} ns");
            assert_eq!(page.players()[0].position, robot.players()[0].position);
        }
    }
    assert!(met > 100, "the steps met {met} times");
    let value = robot.outputs()[0].sample.clone().unwrap().value;
    assert!(matches!(value, Value::Number(_)));
}
