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

Part of the device runtime interfaces, with
[`arora-bridge`](https://crates.io/crates/arora-bridge) and
[`arora-behavior`](https://crates.io/crates/arora-behavior).
