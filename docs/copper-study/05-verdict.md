# 5. Verdict

> Sub-study of [README.md](README.md). The synthesis: switch, reuse, or avoid a
> redundant project?

## Direct answers to the questions asked

**"The HAL, data layer and bridge feel like they should be static — would I gain
using Copper there?"**
No. Those are already static (linked Rust crates). Copper does not supply a HAL,
a data store, or a Studio bridge; it supplies an alternative *driver loop* whose
data model (typed per-edge messages, no shared state) **conflicts** with the
shared-`DataStore` blackboard the whole design rests on. See [01](01-static-core.md).

**"For the more dynamic modules, may Copper help at all?"**
No — Copper has no dynamic loading by design (verified: the graph is fixed at
compile time). Its escape hatches reinvent Arora's own wasm loader inside a task,
minus Arora's type system. See [02](02-dynamic-modules.md).

**"Behavior trees in Copper — competitive?"**
Copper has none. Bolting on `bonsai-bt` gives a fast, static, Rust-only BT that
**cannot** do Arora's dynamic, cross-language, module-backed leaves. Competitive
only if you abandon that capability — and then you'd use bonsai directly, not via
Copper. See [03](03-behavior-trees.md).

**"Is the communication plug-in system worth reusing?"**
Only inside the Copper runtime. The reusable parts (Zenoh transport, storage
plugin, ROS 2-over-Zenoh, per-channel codecs) are available directly via the
`zenoh` crate — the path studio-bridge is already on. See [04](04-communication-plugins.md).

**"At best I gain by avoiding maintaining a redundant project — is it really?"**
No, the gut feeling inverts. **Copper would be the redundant project here**, not
the thing that removes one. Adopting it does not retire Arora (you still need
dynamic modules, which Copper lacks); it adds a second runtime with a conflicting
data model, and its headline value — deterministic replay and zero-alloc sub-µs
scheduling — targets a bottleneck Arora doesn't have at this layer (the static
core is I/O-bound at millisecond rates; the dynamic layer runs at behaviour
rates). You would maintain Arora **and** Copper to get less flexibility.

## Why Copper is still a good framework (just not for this)

None of the above is a knock on Copper. For a **self-contained, hard-real-time,
on-robot control pipeline** (IMU → filter → controller → actuator at kHz), its
strengths are real and were corroborated here: the static DAG compiles and runs
([`sandbox/`](sandbox/)), a full 3-task iteration is ~410 ns, message passing is
zero-copy/zero-serde, and it gives deterministic replay for free. That is a
legitimate tool — as a **separate subsystem**, connected to Arora over Zenoh, *if
and when* Arora grows such a loop. This matches the prior studio-bridge study's
"complementary, not a replacement" conclusion, but narrower: Arora's *static core
is not a control pipeline*, so even the complementary case only opens up for a
future real-time inner loop, not for the HAL/data/bridge.

## Ideas worth stealing (cheap, no framework adoption)

1. **Wiring as data** — Copper's RON graph. Arora already has the analog
   (`module.yaml`, behavior-tree records); keep leaning on it.
2. **Freeze/thaw + unified log for record-replay** — a compelling pattern. A
   lightweight "snapshot + journal the `DataStore`" could give Arora deterministic
   replay of behaviour without Copper.
3. **Native executor on hot paths + shared inter-module buffers** — the two
   levers that actually cut dispatch overhead ([02](02-dynamic-modules.md)); the
   shared-buffer idea is already floated in the split proposal.

## Decision matrix

| Option | Recommendation |
|--------|----------------|
| Replace Arora's engine/core with Copper | **No** — loses dynamic modules; conflicts with `DataStore` |
| Use Copper for the static HAL/data/bridge | **No** — no gain, model mismatch, churn |
| Use Copper for behavior-tree execution | **No** — kills dynamic/cross-language leaves; bonsai needs no Copper |
| Reuse Copper's Zenoh/ROS 2 bridge plug-ins | **No** — coupled to the Copper runtime; use `zenoh` directly |
| Copper as a peer real-time control subsystem over Zenoh | **Maybe later** — only if a hard-real-time inner loop appears |
| Borrow Copper's ideas (wiring-as-data, replay, native/shared-buffer) | **Yes** — cheap, no adoption |

## Recommendation

Proceed with the split as planned ([`../proposal-split-arora-repos.md`](../proposal-split-arora-repos.md),
[`../proposal-bring-studio-bridge-in.md`](../proposal-bring-studio-bridge-in.md)):
keep `arora-sdk::Instance` as the loop, link `arora-ecbs` + `studio-bridge`
directly, keep the dynamic module engine as Arora's core. Do **not** introduce
Copper as the engine or for the static layer. Revisit Copper only as an optional,
Zenoh-connected real-time control subsystem if a kHz on-robot loop becomes a
requirement — and even then, evaluate it against a plain `bonsai-bt` + hand-rolled
loop first.
