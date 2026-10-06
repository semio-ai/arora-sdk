//! The arora-naoqi device end to end against an in-process fake NAOqi: the HAL, the
//! default tree and its leaves (Say, HandsReady, Equals, Assign) on the Arora runtime.

#[allow(dead_code)]
mod fake_naoqi;

use std::time::Duration;

use arora::{Arora, HostModule};
use arora_hal::Hal;
use arora_hal_naoqi::gestures::{naoqi_gestures, HANDS_READY_DURATION};
use arora_hal_naoqi::say::{self, naoqi_say};
use arora_hal_naoqi::{
    ready_tree_initial_state, NaoqiHal, NaoqiRobotConfig, COMMAND_BEHAVIOR, READY_TREE,
};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;
use fake_naoqi::FakeNaoqi;

const STEP: Duration = Duration::from_millis(20);

fn behavior(store: &SimpleDataStore) -> Option<Value> {
    store.read(&[Key::from(COMMAND_BEHAVIOR)]).pop().flatten()
}

/// The default tree says "Arora is ready" while the hands play their gesture, then hands
/// the request back; requested again, it plays again.
#[tokio::test(flavor = "multi_thread")]
async fn the_ready_tree_speaks_moves_the_hands_and_plays_again_on_request() {
    let robot = FakeNaoqi::start().await;
    let hal = NaoqiHal::new(NaoqiRobotConfig::with_url(&robot.url))
        .await
        .expect("connect the HAL");
    say::install(hal.voice());
    let store = SimpleDataStore::new();
    store
        .write(ready_tree_initial_state(&hal.read_all().await.unwrap()))
        .unwrap();
    let ready = || StateChange::set(COMMAND_BEHAVIOR, Value::String("ready".to_string()));
    let mut device = Arora::builder()
        .with_data_store(Box::new(store.clone()))
        .with_hal(Box::new(hal))
        .with_host_module(HostModule::of::<naoqi_say::Module>())
        .with_host_module(HostModule::of::<naoqi_gestures::Module>())
        .with_host_module(HostModule::of::<blackboard::blackboard::Module>())
        .with_groot(READY_TREE)
        .build()
        .expect("the default tree resolves against the device's leaves");

    // Lockstep: tree time advances a step per iteration, the robot answers meanwhile.
    let play = |device: &mut Arora| {
        let steps = HANDS_READY_DURATION.as_millis() / STEP.as_millis() + 50;
        for _ in 0..steps {
            device.step(STEP).expect("step");
            std::thread::sleep(Duration::from_millis(2));
            if behavior(&store) == Some(Value::String("idle".to_string())) {
                return;
            }
        }
        panic!(
            "the ready behavior did not hand the request back: behavior error {:?}, \
             command.behavior {:?}, said {:?}, l_hand target {:?}",
            device
                .behavior_error()
                .borrow()
                .as_ref()
                .map(|e| e.to_string()),
            behavior(&store),
            robot.said.lock().unwrap(),
            store.read(&[Key::from("l_hand.target_position")]),
        );
    };
    play(&mut device);

    assert_eq!(
        robot.said.lock().unwrap().as_slice(),
        ["Arora is ready".to_string()]
    );
    let moved: Vec<String> = robot
        .set_angles
        .lock()
        .unwrap()
        .iter()
        .flat_map(|(names, _, _)| names.clone())
        .collect();
    for joint in ["LWristYaw", "RWristYaw", "LHand", "RHand"] {
        assert!(moved.iter().any(|name| name == joint), "{joint} moved");
    }
    let stiffened = robot.set_stiffnesses.lock().unwrap().clone();
    let last_stiffness = stiffened
        .iter()
        .rev()
        .find_map(|(names, values)| {
            names
                .iter()
                .position(|name| name == "LHand")
                .map(|index| values[index])
        })
        .expect("the hand was stiffened");
    assert_eq!(last_stiffness, 0.0, "the hands are relaxed once over");

    // Requested again, after a pause: it speaks again.
    std::thread::sleep(say::IDLE_STOP + Duration::from_millis(50));
    store.write(ready()).unwrap();
    play(&mut device);
    assert_eq!(robot.said.lock().unwrap().len(), 2);
}
