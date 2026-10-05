//! NAOqi robots (NAO, Pepper) as Arora HALs, over the `qi` framework.
//!
//! One [`arora_hal::Hal`] implementation, [`NaoqiHal`], connects to a robot's NAOqi with
//! [`libqi-rs`](https://github.com/victorpaleologue/libqi-rs-vibe) and exposes it as an Arora
//! device: joint states, battery, inertial unit, sonars and touch sensors flow in as keys;
//! joint targets, stiffness, LEDs and base velocity commands flow out to the NAOqi services
//! (`ALMotion`, `ALLeds`). The keys are those of an Arora robot [`standard`], local to this
//! HAL; [`keys`] maps NAOqi onto them.
//!
//! Behavior trees reach the rest of the robot through leaves: [`say`] speaks in the robot's
//! voice (`ALTextToSpeech`), [`gestures`] play timed joint targets.
//!
//! The robot is described by a [`NaoqiRobotConfig`] (address, credentials, joint id
//! mapping, sampling period); `configs/nao.json` is a sample.

mod config;
pub use config::{JointIdMapping, NaoqiRobotConfig, SensorsConfig};

mod conversions;
pub use conversions::keys;

mod naoqi_error;
pub use naoqi_error::NaoqiRobotError;

mod naoqi_hal;
pub use naoqi_hal::{NaoqiHal, Voice};

pub mod gestures;
pub mod say;
pub mod standard;

/// The default behavior of `arora-naoqi`, a Groot tree: while `command.behavior` is
/// `ready`, the robot says "Arora is ready" as its hands play a gesture, then hands the
/// request back (`idle`). Its leaves are [`say`], [`gestures`] and the blackboard's
/// `Equals` and `Assign`.
pub const READY_TREE: &str = include_str!("../trees/ready.groot.xml");

/// The key the default behavior switches on.
pub const COMMAND_BEHAVIOR: &str = "command.behavior";

/// The joints the default behavior's gesture moves, by their default ids.
pub const READY_TREE_JOINTS: [&str; 4] = ["l_wrist_yaw", "r_wrist_yaw", "l_hand", "r_hand"];

/// The keys the default behavior's leaves write, seeded so that they bind: a tree's
/// out-parameter binds to a key that exists. Joint targets start where the robot is
/// (`state`, its HAL's [`read_all`](arora_hal::Hal::read_all)), so that writing them
/// moves nothing. `command.behavior` starts at `ready`: the behavior plays at start.
pub fn ready_tree_initial_state(
    state: &arora_types::data::State,
) -> arora_types::data::StateChange {
    use arora_types::data::{Key, StateChange};
    use arora_types::value::Value;

    let measured = |joint: &str, component: &str| {
        state
            .get(&Key::from(format!("{joint}.{component}")))
            .cloned()
            .flatten()
            .unwrap_or(Value::F64(0.0))
    };
    let mut change = StateChange::set(COMMAND_BEHAVIOR, Value::String("ready".to_string()));
    change.set.insert(
        Key::from("speech.viseme"),
        Some(Value::String(say::SILENCE_VISEME.to_string())),
    );
    for joint in READY_TREE_JOINTS {
        change.set.insert(
            Key::from(format!("{joint}.{}", keys::TARGET_POSITION)),
            Some(measured(joint, keys::POSITION)),
        );
        change.set.insert(
            Key::from(format!("{joint}.{}", keys::TARGET_STIFFNESS)),
            Some(measured(joint, keys::STIFFNESS)),
        );
    }
    change
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nao_sample_config_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/configs/nao.json");
        let file = std::fs::File::open(path).expect("nao sample config should exist");
        let config: NaoqiRobotConfig =
            serde_json::from_reader(file).expect("nao sample config should parse");
        assert!(config.validate().is_ok());
        assert_eq!(config.model_family.as_deref(), Some("nao"));
    }
}
