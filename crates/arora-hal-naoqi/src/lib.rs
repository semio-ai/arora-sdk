//! NAOqi robots (NAO, Pepper) as Arora HALs, over the `qi` framework.
//!
//! One [`arora_hal::Hal`] implementation, [`NaoqiHal`], connects to a robot's NAOqi with
//! [`libqi-rs`](https://github.com/victorpaleologue/libqi-rs-vibe) and exposes it as an Arora
//! device: joint states, battery, inertial unit, sonars and touch sensors flow in as keys;
//! joint targets, stiffness, speech, LEDs and base velocity commands flow out to the
//! NAOqi services (`ALMotion`, `ALTextToSpeech`, `ALLeds`). The key vocabulary is in
//! [`keys`].
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
pub use naoqi_hal::NaoqiHal;

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
