//! The NAOqi HAL against an in-process fake NAOqi.

mod fake_naoqi;

use std::time::Duration;

use arora_behavior::Status;
use arora_hal::{Hal, UpdatesStream};
use arora_hal_naoqi::say::{self, naoqi_say};
use arora_hal_naoqi::{keys, NaoqiHal, NaoqiRobotConfig};
use arora_types::data::{Key, StateChange};
use arora_types::value::Value;
use fake_naoqi::FakeNaoqi;
use futures::StreamExt;
use tokio::time::timeout;

const TIMEOUT: Duration = Duration::from_secs(5);

async fn start() -> (FakeNaoqi, NaoqiHal) {
    let robot = FakeNaoqi::start().await;
    let mut config = NaoqiRobotConfig::with_url(&robot.url);
    config.sensor_period_ms = 10;
    let hal = NaoqiHal::new(config).await.expect("connect the HAL");
    (robot, hal)
}

/// Waits for an update satisfying the predicate, returning the value of the key in it.
async fn wait_for(
    feed: &mut UpdatesStream,
    key: &str,
    predicate: impl Fn(&Option<Value>) -> bool,
) -> Option<Value> {
    let key = Key::from(key);
    timeout(TIMEOUT, async {
        while let Some(change) = feed.next().await {
            if let Some(value) = change.set.get(&key) {
                if predicate(value) {
                    return value.clone();
                }
            }
        }
        panic!("the updates feed ended");
    })
    .await
    .unwrap_or_else(|_| panic!("no update of {key:?} matched in time"))
}

/// Waits for an update satisfying the predicate and returns it.
async fn wait_for_change(
    feed: &mut UpdatesStream,
    predicate: impl Fn(&StateChange) -> bool,
) -> StateChange {
    timeout(TIMEOUT, async {
        while let Some(change) = feed.next().await {
            if predicate(&change) {
                return change;
            }
        }
        panic!("the updates feed ended");
    })
    .await
    .expect("no update matched in time")
}

fn f64_of(value: &Option<Value>) -> Option<f64> {
    match value {
        Some(Value::F64(v)) => Some(*v),
        _ => None,
    }
}

#[tokio::test]
async fn describes_the_robot() {
    let (_robot, hal) = start().await;
    let description = hal.describe().await;
    assert_eq!(description.model_family.as_deref(), Some("nao"));
    assert_eq!(description.software_version.as_deref(), Some("2.8.7.4"));
    assert_eq!(description.hardware_version.as_deref(), Some("V6"));
}

#[tokio::test]
async fn turns_autonomous_life_off_when_connecting() {
    let (robot, _hal) = start().await;
    assert_eq!(*robot.life_state.lock().unwrap(), "disabled");
}

#[tokio::test]
async fn leaves_autonomous_life_on_when_configured_to() {
    let robot = FakeNaoqi::start().await;
    let mut config = NaoqiRobotConfig::with_url(&robot.url);
    config.disable_autonomous_life = false;
    let _hal = NaoqiHal::new(config).await.expect("connect the HAL");
    assert_eq!(*robot.life_state.lock().unwrap(), "solitary");
}

#[tokio::test]
async fn reports_sensors_as_keys() {
    let (robot, hal) = start().await;
    let mut feed = hal.updates();

    // The first update is the snapshot of the initial sample.
    let snapshot = timeout(TIMEOUT, feed.next()).await.unwrap().unwrap();
    assert_eq!(
        f64_of(snapshot.set.get(&Key::from("head_pitch.position")).unwrap()),
        Some(0.1f32 as f64)
    );
    assert_eq!(
        f64_of(
            snapshot
                .set
                .get(&Key::from("l_shoulder_pitch.stiffness"))
                .unwrap()
        ),
        Some(0.8f32 as f64)
    );
    assert_eq!(
        f64_of(snapshot.set.get(&Key::from(keys::BATTERY_CHARGE)).unwrap()),
        Some(0.75)
    );
    assert_eq!(
        snapshot
            .set
            .get(&Key::from(keys::BATTERY_CHARGING))
            .unwrap(),
        &Some(Value::Boolean(false))
    );
    assert!(snapshot.set.contains_key(&Key::from("imu.angle.x")));
    assert!(snapshot.set.contains_key(&Key::from("sonar.left.distance")));
    // Keys the robot does not have are absent, not reported as invalid.
    assert!(!snapshot
        .set
        .contains_key(&Key::from("sonar.right.distance")));
    // Touch keys start from the current value of their event.
    wait_for(&mut feed, "touch.head.front", |v| {
        v == &Some(Value::Boolean(false))
    })
    .await;

    // Sensor changes are reported, with velocities derived from consecutive samples.
    robot.set_memory("Device/SubDeviceList/HeadYaw/Position/Sensor/Value", 0.5);
    let change = wait_for_change(&mut feed, |change| {
        change
            .set
            .get(&Key::from("head_yaw.position"))
            .is_some_and(|v| f64_of(v) == Some(0.5))
    })
    .await;
    let velocity = change.set.get(&Key::from("head_yaw.velocity")).unwrap();
    assert!(f64_of(velocity).unwrap() > 0.0, "{velocity:?}");

    // The cache follows.
    assert_eq!(
        hal.read(&[Key::from("head_yaw.position"), Key::from("nope")])
            .await
            .unwrap(),
        vec![Some(Value::F64(0.5)), None]
    );
}

#[tokio::test]
async fn touch_events_drive_boolean_keys() {
    let (robot, hal) = start().await;
    let mut feed = hal.updates();
    // Let the event subscriptions settle.
    tokio::time::sleep(Duration::from_millis(100)).await;
    robot.raise_event("FrontTactilTouched", 1.0);
    let touched = wait_for(&mut feed, "touch.head.front", |v| {
        v == &Some(Value::Boolean(true))
    })
    .await;
    assert_eq!(touched, Some(Value::Boolean(true)));
    robot.raise_event("FrontTactilTouched", 0.0);
    wait_for(&mut feed, "touch.head.front", |v| {
        v == &Some(Value::Boolean(false))
    })
    .await;
    robot.raise_event("RightBumperPressed", 1.0);
    wait_for(&mut feed, "bumper.right", |v| {
        v == &Some(Value::Boolean(true))
    })
    .await;
}

#[tokio::test]
async fn writes_joint_targets_and_stiffness() {
    let (robot, hal) = start().await;
    let mut feed = hal.updates();
    let mut change = StateChange::set("head_yaw.target_position", Value::F64(0.7));
    change.set.insert(
        Key::from("l_shoulder_pitch.target_position"),
        Some(Value::F32(-0.2)),
    );
    change.set.insert(
        Key::from("head_yaw.target_stiffness"),
        Some(Value::F64(1.0)),
    );
    change
        .set
        .insert(Key::from("unknown.target_position"), Some(Value::F64(1.0)));
    hal.write(change).await.unwrap();

    let mut commands = robot.set_angles.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    let (mut names, mut angles, speed) = commands.remove(0);
    let mut pairs: Vec<_> = names.drain(..).zip(angles.drain(..)).collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        pairs,
        [
            ("HeadYaw".to_string(), 0.7f32),
            ("LShoulderPitch".to_string(), -0.2f32)
        ]
    );
    assert_eq!(speed, 0.3);
    assert_eq!(
        robot.set_stiffnesses.lock().unwrap().as_slice(),
        [(vec!["HeadYaw".to_string()], vec![1.0f32])]
    );
    // The commanded position is sensed back through the sampling.
    wait_for(&mut feed, "head_yaw.position", |v| {
        f64_of(v) == Some(0.7f32 as f64)
    })
    .await;
    // Written keys are readable.
    assert_eq!(
        hal.read(&[Key::from("head_yaw.target_position")])
            .await
            .unwrap(),
        vec![Some(Value::F64(0.7))]
    );
}

/// The `Say` leaf speaks in the robot's voice: running while `ALTextToSpeech.say` has not
/// returned, then succeeding while ticked, the sentence said once; ticked again after a
/// halt, it speaks again.
#[tokio::test(flavor = "multi_thread")]
async fn say_speaks_in_the_robots_voice() {
    let (robot, hal) = start().await;
    say::install(hal.voice());
    let mut viseme = String::new();
    let mut tick = || naoqi_say::say("hello".to_string(), String::new(), &mut viseme);
    assert_eq!(tick(), Status::Running);
    timeout(TIMEOUT, async {
        while tick() != Status::Success {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the sentence is said");
    assert_eq!(tick(), Status::Success, "latched while ticked");
    assert_eq!(robot.said.lock().unwrap().as_slice(), ["hello".to_string()]);
    assert_eq!(viseme, say::SILENCE_VISEME);

    // Halted, then ticked again: a new utterance.
    tokio::time::sleep(say::IDLE_STOP + Duration::from_millis(50)).await;
    let mut viseme = String::new();
    assert_eq!(
        naoqi_say::say("hello".to_string(), String::new(), &mut viseme),
        Status::Running
    );
    timeout(TIMEOUT, async {
        while robot.said.lock().unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the sentence is said again");
}

#[tokio::test]
async fn writes_leds_and_base_velocity() {
    let (robot, hal) = start().await;
    let mut change = StateChange::set("led.FaceLeds.color", Value::U32(0x00ff_0000));
    change
        .set
        .insert(Key::from(keys::VELOCITY_X), Some(Value::F64(0.5)));
    hal.write(change).await.unwrap();
    hal.write(StateChange::set(keys::ROTATION_Z, Value::F64(-0.25)))
        .await
        .unwrap();
    assert_eq!(
        robot.leds.lock().unwrap().as_slice(),
        [("FaceLeds".to_string(), 0x00ff_0000i32, 0.0f32)]
    );
    assert_eq!(
        robot.move_toward.lock().unwrap().as_slice(),
        [(0.5f32, 0.0, 0.0), (0.5, 0.0, -0.25)]
    );
}

#[tokio::test]
async fn connection_failures_are_reported() {
    let error = NaoqiHal::new(NaoqiRobotConfig::with_url("tcp://127.0.0.1:1"))
        .await
        .err()
        .expect("no NAOqi listens on port 1");
    assert!(error.to_string().contains("connection"), "{error}");
}
