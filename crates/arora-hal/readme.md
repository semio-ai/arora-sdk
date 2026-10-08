# arora-hal

The hardware abstraction layer of [Arora](https://github.com/semio-ai/arora-sdk):
a device's sensors and actuators presented as typed state.

A `Hal` describes the device (`HalDescription`: model family, hardware and
software versions), exposes its live values by key (`read`/`read_all`/`write`),
and streams hardware-initiated updates (`updates`). `HalAssets` adds the
device's 3D model (`model_glb`) so a remote can render what it is controlling.

The runtime drains HAL updates into the shared blackboard each step and flushes
state changes back — behaviors never talk to hardware directly. `FakeHal` is an
in-memory implementation for tests and hardware-less runs.

Each HAL states the keys it reads and writes. `FakeHal`, `arora-hal-ros2` and
`arora-hal-restful` name a joint's values with an attribute on the key's last
segment: `<joint>.target_position` is a setpoint written to the hardware,
`<joint>.position` what is sensed back. That is their naming contract, not
something keys require: to the store and the bridges a key is a `/`-separated
path, and `.` one more character of its name.

Part of the device runtime interfaces, with
[`arora-bridge`](https://docs.rs/arora-bridge) and
[`arora-behavior`](https://docs.rs/arora-behavior).
