# arora-bridge

The live-control link of [Arora](https://github.com/semio-ai/arora-sdk): how a
running device is reached from the outside.

A `Bridge` carries three streams between the device and a remote — device-info
updates, a data-interest toggle, and commands — plus `send_data` to push state
changes out. Commands are `BridgeOp`s: read keys (`Get`), apply a state change
(`Update`), call a function (`Call`), and introspect what exists (`ListKeys`,
`DescribeMethods` — the live-edit surface).

The runtime answers commands against its state and engine, one per step, and
replies through the command's channel. `FakeBridge` is the no-op implementation
for offline runs; Semio Studio's Zenoh connector and Vizij's WebSocket server
are real ones.

The client side is here too: `Caller` carries a call to a device, and the
`client` module holds the conventions any client speaks over these ops —
calling a method by name, starting and halting a task run, and the shapes keys
and methods are read in — so a remote over a bridge and the device's own
in-process caller behave alike.

Part of the device runtime interfaces, with
[`arora-hal`](https://docs.rs/arora-hal) and
[`arora-behavior`](https://docs.rs/arora-behavior).
