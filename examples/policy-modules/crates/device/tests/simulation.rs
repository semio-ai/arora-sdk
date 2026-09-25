//! The device in lockstep: the policies balance and walk the simulated duck,
//! from the wasm guest and in-process alike, and the trees switch between
//! them. Each test builds its own device from the fetched assets
//! (`tools/fetch-assets.sh`; MuJoCo per `tools/setup-mujoco.sh`).

use std::time::Instant;

use arora_hal_mujoco::Clock;
use policy_device::{default_model, tree_path, Command, Device, DeviceConfig, Executor};

fn device(tree: &str, executor: Executor, command: Command) -> Device {
    let config = DeviceConfig {
        model: default_model(),
        tree: std::fs::read_to_string(tree_path(tree)).expect("the tree file exists"),
        executor,
        clock: Clock::Lockstep,
        record: None,
        command,
    };
    Device::build(config, None).expect("the device builds")
}

/// Standing still for ten seconds, the duck keeps its height and stays upright.
#[test]
fn stands_in_place() {
    let mut device = device("stand", Executor::Native, Command::default());
    let outcome = device.run_for(10.0).unwrap();
    assert!(!outcome.fell, "{outcome:?}");
    assert!(outcome.min_height > 0.10, "{outcome:?}");
    assert!(outcome.distance < 0.05, "{outcome:?}");
}

/// Commanded forward at 0.3 m/s, the duck walks a good part of a metre in
/// ten seconds without falling.
#[test]
fn walks_forward_on_command() {
    let mut device = device(
        "walk",
        Executor::Native,
        Command {
            vx: 0.3,
            ..Command::default()
        },
    );
    let outcome = device.run_for(10.0).unwrap();
    assert!(!outcome.fell, "{outcome:?}");
    assert!(outcome.distance > 0.5, "{outcome:?}");
    assert!(outcome.position[0] > 0.4, "walked forward: {outcome:?}");
}

/// The same walk from the wasm guest: the module ships as wasm and behaves
/// as it does in-process.
#[test]
fn the_wasm_guest_walks_too() {
    let started = Instant::now();
    let mut device = device(
        "walk",
        Executor::Wasm,
        Command {
            vx: 0.3,
            ..Command::default()
        },
    );
    eprintln!("wasm device built in {:.1?}", started.elapsed());
    let started = Instant::now();
    let outcome = device.run_for(6.0).unwrap();
    eprintln!("6 s simulated in {:.1?}", started.elapsed());
    assert!(!outcome.fell, "{outcome:?}");
    assert!(outcome.distance > 0.25, "{outcome:?}");
}

/// Dropping the command mid-run hands the legs from the walking network to
/// the standing one: the duck stops.
#[test]
fn stops_when_the_command_drops() {
    let mut device = device(
        "walk",
        Executor::Native,
        Command {
            vx: 0.3,
            ..Command::default()
        },
    );
    let walking = device.run_for(6.0).unwrap();
    assert!(walking.distance > 0.25, "{walking:?}");
    device.set_command(Command::default()).unwrap();
    device.run_for(2.0).unwrap();
    let (before, _) = device.ground_truth();
    let standing = device.run_for(4.0).unwrap();
    assert!(!standing.fell, "{standing:?}");
    let (after, _) = device.ground_truth();
    let drift = ((after[0] - before[0]).powi(2) + (after[1] - before[1]).powi(2)).sqrt();
    assert!(drift < 0.15, "drifted {drift} m after the command dropped");
}

/// The showcase tree: a fallback over a fallen check, and a parallel of a
/// head animation over the walking leaf — it loads, runs, and the duck walks
/// while looking around.
#[test]
fn the_showcase_tree_walks_while_animating_the_head() {
    let mut device = device(
        "showcase",
        Executor::Native,
        Command {
            vx: 0.3,
            ..Command::default()
        },
    );
    let outcome = device.run_for(8.0).unwrap();
    assert!(!outcome.fell, "{outcome:?}");
    assert!(outcome.distance > 0.3, "{outcome:?}");
}

/// The posture sequence: sit, rise, then stand — the duck ends up standing.
#[test]
fn sits_rises_and_stands() {
    let mut device = device("sit_rise", Executor::Native, Command::default());
    // The sit leaf runs 3 s: within them the base comes down to the seat.
    let sitting = device.run_for(3.0).unwrap();
    assert!(sitting.min_height < 0.09, "sat down: {sitting:?}");
    // Then the rise leaf (3 s) and the stand leaf: up and upright again.
    let risen = device.run_for(7.0).unwrap();
    assert!(!risen.fell, "{risen:?}");
    assert!(risen.position[2] > 0.10, "standing again: {risen:?}");
}

/// A learned skill then a stand: the kick ends and the duck is still up.
#[test]
fn kicks_then_stands() {
    let mut device = device("kick", Executor::Native, Command::default());
    let outcome = device.run_for(4.0).unwrap();
    assert!(!outcome.fell, "{outcome:?}");
    assert!(outcome.position[2] > 0.10, "{outcome:?}");
}
