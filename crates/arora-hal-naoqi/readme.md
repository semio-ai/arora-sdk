# arora-hal-naoqi

NAO and Pepper robots as Arora devices, over their NAOqi middleware.

The HAL talks to the robot with [`libqi-rs`](https://github.com/victorpaleologue/libqi-rs-vibe),
a Rust implementation of the `qi` framework: no C++ SDK, no cross-compilation, the HAL
runs on any host that reaches the robot over TCP (port 9559).

```sh
cargo run --features runner --bin arora-naoqi -- tcp://nao.local:9559
# or with a configuration file (see configs/nao.json)
cargo run --features runner --bin arora-naoqi -- configs/nao.json
```

## Keys

Joint ids are the NAOqi joint names in snake case (`HeadYaw` → `head_yaw`,
`LShoulderPitch` → `l_shoulder_pitch`), unless the configuration overrides them.

| Key | Direction | NAOqi source or target |
|---|---|---|
| `<joint_id>.position` (rad), `.velocity` (rad/s, derived), `.stiffness`, `.temperature` (°C), `.current` (A) | sensed | `ALMemory` `Device/SubDeviceList/<Joint>/…` |
| `battery.charge` (0..1), `battery.current` (A), `battery.charging` | sensed | `ALMemory` battery keys |
| `imu.angle.{x,y}`, `imu.gyroscope.{x,y,z}`, `imu.accelerometer.{x,y,z}` | sensed | `ALMemory` inertial sensor keys |
| `sonar.{left,right}.distance` (m) | sensed | `ALMemory` ultrasound keys |
| `touch.head.{front,middle,rear}`, `touch.hand.{left,right}.{back,left,right}`, `bumper.{left,right}`, `chest_button` | sensed (events) | `ALMemory` tactile events |
| `<joint_id>.target_position` (rad) | commanded | `ALMotion.setAngles` |
| `<joint_id>.target_stiffness` (0..1) | commanded | `ALMotion.setStiffnesses` |
| `text` | commanded | `ALTextToSpeech.say`; the key is unset once spoken so the same text can be said again |
| `led.<group>.color` (`0xRRGGBB`), `led.<group>.intensity` | commanded | `ALLeds.fadeRGB`, `ALLeds.setIntensity` |
| `velocity.{x,y}`, `rotation.z` (normalized -1..1) | commanded | `ALMotion.moveToward` |

Numeric sensor keys are sampled from `ALMemory` at `sensor_period_ms` (50 ms by default)
in one `getListData` call, and only changed values are reported. Touch keys are driven
by `ALMemory` events.
