//! The Arora key vocabulary of NAOqi robots.
//!
//! Keys follow the `entity.component` grammar of Arora: joints are entities named after
//! their NAOqi name in snake case (`head_yaw`), the other entities are fixed.

/// The text to say (`ALTextToSpeech.say`); unset by the HAL once spoken.
pub const TEXT: &str = "text";
/// The battery charge, from 0 to 1.
pub const BATTERY_CHARGE: &str = "battery.charge";
/// The battery current in amperes, positive when charging.
pub const BATTERY_CURRENT: &str = "battery.current";
/// Whether the battery is charging.
pub const BATTERY_CHARGING: &str = "battery.charging";
/// The forward velocity command of the base, normalized from -1 to 1.
pub const VELOCITY_X: &str = "velocity.x";
/// The lateral velocity command of the base, normalized from -1 to 1.
pub const VELOCITY_Y: &str = "velocity.y";
/// The rotation velocity command of the base, normalized from -1 to 1.
pub const ROTATION_Z: &str = "rotation.z";
/// The entity of LED groups: `led.<group>.color`, `led.<group>.intensity`.
pub const LED_ENTITY: &str = "led";

/// The component of a joint key holding its measured position, in radians.
pub const POSITION: &str = "position";
/// The component of a joint key holding its velocity, in radians per second.
pub const VELOCITY: &str = "velocity";
/// The component of a joint key holding its stiffness, from 0 to 1.
pub const STIFFNESS: &str = "stiffness";
/// The component of a joint key holding its temperature, in degrees Celsius.
pub const TEMPERATURE: &str = "temperature";
/// The component of a joint key holding its electric current, in amperes.
pub const CURRENT: &str = "current";
/// The component of a joint key commanding its position, in radians.
pub const TARGET_POSITION: &str = "target_position";
/// The component of a joint key commanding its stiffness, from 0 to 1.
pub const TARGET_STIFFNESS: &str = "target_stiffness";

/// The `ALMemory` keys of the battery, with the Arora keys they feed.
pub const BATTERY_MEMORY_KEYS: &[(&str, &str)] = &[
    (
        "Device/SubDeviceList/Battery/Charge/Sensor/Value",
        BATTERY_CHARGE,
    ),
    (
        "Device/SubDeviceList/Battery/Current/Sensor/Value",
        BATTERY_CURRENT,
    ),
];

/// The `ALMemory` keys of the inertial unit, with the Arora keys they feed.
pub const INERTIAL_MEMORY_KEYS: &[(&str, &str)] = &[
    (
        "Device/SubDeviceList/InertialSensor/AngleX/Sensor/Value",
        "imu.angle.x",
    ),
    (
        "Device/SubDeviceList/InertialSensor/AngleY/Sensor/Value",
        "imu.angle.y",
    ),
    (
        "Device/SubDeviceList/InertialSensor/GyroscopeX/Sensor/Value",
        "imu.gyroscope.x",
    ),
    (
        "Device/SubDeviceList/InertialSensor/GyroscopeY/Sensor/Value",
        "imu.gyroscope.y",
    ),
    (
        "Device/SubDeviceList/InertialSensor/GyroscopeZ/Sensor/Value",
        "imu.gyroscope.z",
    ),
    (
        "Device/SubDeviceList/InertialSensor/AccelerometerX/Sensor/Value",
        "imu.accelerometer.x",
    ),
    (
        "Device/SubDeviceList/InertialSensor/AccelerometerY/Sensor/Value",
        "imu.accelerometer.y",
    ),
    (
        "Device/SubDeviceList/InertialSensor/AccelerometerZ/Sensor/Value",
        "imu.accelerometer.z",
    ),
];

/// The `ALMemory` keys of the sonars, with the Arora keys they feed.
pub const SONAR_MEMORY_KEYS: &[(&str, &str)] = &[
    (
        "Device/SubDeviceList/US/Left/Sensor/Value",
        "sonar.left.distance",
    ),
    (
        "Device/SubDeviceList/US/Right/Sensor/Value",
        "sonar.right.distance",
    ),
];

/// The `ALMemory` touch events, with the boolean Arora keys they drive.
pub const TOUCH_EVENTS: &[(&str, &str)] = &[
    ("FrontTactilTouched", "touch.head.front"),
    ("MiddleTactilTouched", "touch.head.middle"),
    ("RearTactilTouched", "touch.head.rear"),
    ("HandLeftBackTouched", "touch.hand.left.back"),
    ("HandLeftLeftTouched", "touch.hand.left.left"),
    ("HandLeftRightTouched", "touch.hand.left.right"),
    ("HandRightBackTouched", "touch.hand.right.back"),
    ("HandRightLeftTouched", "touch.hand.right.left"),
    ("HandRightRightTouched", "touch.hand.right.right"),
    ("LeftBumperPressed", "bumper.left"),
    ("RightBumperPressed", "bumper.right"),
    ("ChestButtonPressed", "chest_button"),
];

/// The Arora joint id of a NAOqi joint name: `LShoulderPitch` → `l_shoulder_pitch`.
pub fn joint_id_from_naoqi_name(name: &str) -> String {
    let mut id = String::with_capacity(name.len() + 4);
    for (index, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if index > 0 {
                id.push('_');
            }
            id.push(c.to_ascii_lowercase());
        } else {
            id.push(c);
        }
    }
    id
}

/// The `ALMemory` key of a joint sensor or actuator value.
pub fn joint_memory_key(joint: &str, path: &str) -> String {
    format!("Device/SubDeviceList/{joint}/{path}/Value")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joint_ids_are_snake_case() {
        assert_eq!(joint_id_from_naoqi_name("HeadYaw"), "head_yaw");
        assert_eq!(
            joint_id_from_naoqi_name("LShoulderPitch"),
            "l_shoulder_pitch"
        );
        assert_eq!(joint_id_from_naoqi_name("LHipYawPitch"), "l_hip_yaw_pitch");
        assert_eq!(joint_id_from_naoqi_name("RHand"), "r_hand");
    }

    #[test]
    fn memory_keys_follow_the_device_tree() {
        assert_eq!(
            joint_memory_key("HeadYaw", "Position/Sensor"),
            "Device/SubDeviceList/HeadYaw/Position/Sensor/Value"
        );
    }
}
