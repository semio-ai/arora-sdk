# arora-behavior

The behavior abstraction of [Arora](https://github.com/semio-ai/arora-sdk):
anything the runtime can tick.

A `Behavior` advances one step at a time — `tick(&mut BehaviorContext)` — and
reports whether it is `Running` or `Done`. The context hands it the shared
data store (read inputs, write intent) and the module-call bridge, so a
behavior can be a behavior tree
([`arora-behavior-tree`](https://docs.rs/arora-behavior-tree)), a
node graph, or any other interpreter: the runtime queues `Box<dyn Behavior>`
and ticks them without knowing which is which.

Part of the device runtime interfaces, with
[`arora-hal`](https://docs.rs/arora-hal) and
[`arora-bridge`](https://docs.rs/arora-bridge).
