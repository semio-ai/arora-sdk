# arora-hal

The hardware abstraction layer of [Arora](https://github.com/semio-ai/arora-sdk):
a device's sensors and actuators presented as typed state.

A `Hal` describes the device (`HalDescription`: model family, hardware and
software versions), exposes its live values by key (`read`/`read_all`/`write`),
and streams hardware-initiated updates (`updates`). `HalAssets` states the 3D
model of each of the HAL's components — its published reference, content hash,
whether the device serves its bytes, and where it is mounted — so a remote can
render what it is controlling; a HAL hands it to the runtime through
`Hal::assets`, and the runtime serves it as the HAL module's functions, under
the ids in `hal_module`.

The runtime drains HAL updates into the shared blackboard each step and flushes
state changes back — behaviors never talk to hardware directly. `FakeHal` is an
in-memory implementation for tests and hardware-less runs.

Each HAL states the keys it reads and writes. `FakeHal`, `arora-hal-ros2` and
`arora-hal-restful` share a naming contract for a joint's values:
`<joint>.position` is what is sensed back, and its target — the setpoint
written to the hardware — is `<joint>/target.position` or
`<joint>.target_position`. `set_targets` reads both forms; a change that sets
both for one joint applies `<joint>/target.position`, and `fold_newest_wins`
merges a backlog of changes so the later target wins in either form. That is
the HALs' contract, not something keys require: to the store and the bridges a
key is a `/`-separated path, and `.` one more character of its name.

Part of the device runtime interfaces, with
[`arora-bridge`](https://docs.rs/arora-bridge) and
[`arora-behavior`](https://docs.rs/arora-behavior).
