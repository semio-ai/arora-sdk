# arora-bridge

The live-control link of [Arora](https://github.com/semio-ai/arora-sdk): how a
running device is reached from the outside.

A `Bridge` carries three streams between the device and a remote — device-info
updates, a data-interest toggle, and commands — plus `send_data` to push state
changes out. Commands are `BridgeOp`s: read keys (`Get`), apply a state change
(`Update`), call a function (`Call`), and introspect what exists (`ListKeys`,
`ListMethods` — the live-edit surface).

The runtime answers commands against its state and engine, one per step, and
replies through the command's channel. `FakeBridge` is the no-op implementation
for offline runs; Semio Studio's Zenoh connector and Vizij's WebSocket server
are real ones.

Part of the device runtime interfaces, with
[`arora-hal`](https://docs.rs/arora-hal) and
[`arora-behavior`](https://docs.rs/arora-behavior).
