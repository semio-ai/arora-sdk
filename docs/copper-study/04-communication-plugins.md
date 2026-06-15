# 4. Communication plug-ins: Zenoh / ROS 2 bridges

> Sub-study of [README.md](README.md). Question: *the communication plug-in
> system — is it worth reusing?*

## TL;DR

**Only if you adopt the Copper runtime.** Copper's bridges (`cu-zenoh-bridge`,
`cu-ros2-bridge`, `cu-zenoh-sink`, all `1.0.0-rc2`) are `CuBridge`
implementations wired into a Copper task graph — they move **typed edge
payloads**, not "mirror a key-value store with snapshot + liveliness," which is
what `studio-bridge` does. Outside a Copper graph there is nothing to plug them
into, and the raw `zenoh` crate (which Arora/studio-bridge already targets) is
simpler. The reusable thing is the **approach** (Zenoh transport, ROS 2 as
CDR-over-Zenoh, multi-format wire codecs) — and that is available without Copper.

## What the bridges are (confirmed, not run)

> Grounding: crates.io and docs.rs for the `1.0.0-rc2` crates. I did **not**
> compile these (they pull the full Zenoh stack); existence, versions and the
> public surface are from the published metadata/docs.

- `cu-zenoh-bridge = "1.0.0-rc2"` — "Copper bridge for bidirectional Zenoh
  messaging." Public surface: a `ZenohBridge` type and a `WireFormat` enum;
  dependencies show **bincode**, **serde_json**, and **minicbor-serde** → wire
  formats bincode / JSON / CBOR. Depends on `cu29 1.0.0-rc2`.
- `cu-ros2-bridge = "1.0.0-rc2"` — "Copper bridge for ROS 2 messaging over Zenoh
  transport" (CDR encoding, DDS discovery via Zenoh).
- `cu-zenoh-sink = "1.0.0-rc2"` — a Zenoh **sink** task.

The defining fact: each depends on `cu29` and implements Copper's `CuBridge`
(the runtime re-exports `cu29_runtime::cubridge` from its prelude). A bridge is
declared in the **RON** alongside tasks, with `Tx`/`Rx` **channels** mapped to
Zenoh key expressions, and it exchanges the graph's **typed messages**. In other
words it is not a standalone library you call — it is a node in a Copper DAG.

## Why that doesn't fit Arora's bridge

`studio-bridge` is a connector with a different job
([`../proposal-bring-studio-bridge-in.md`](../proposal-bring-studio-bridge-in.md) §2-3):

- it **mirrors a `DataStore` (key/value blackboard) to Semio Studio**, both ways;
- over **Firestore + Zenoh**, with **snapshot-on-connect** and **liveliness
  tokens** (the bridge's current `sem-136` / `sem-137` work);
- it is **async** and network-rate.

`cu-zenoh-bridge` does none of this. It publishes/subscribes typed payloads on
key expressions for a Copper graph; it has no concept of a store to mirror, of a
device snapshot, or of Studio's liveliness model. Reusing it would mean:

1. adopting the Copper runtime (so there is a graph to bridge), and
2. re-expressing the store-mirroring/snapshot/liveliness logic on top of typed
   channels anyway.

That is more work than the current path and discards the bridge semantics Arora
actually needs.

## What *is* worth taking — without Copper

The plug-in **system** is not reusable in isolation, but the choices it encodes
are sound and independent of Copper:

- **Zenoh as the transport + the storage plugin** for a local cache. This is
  already the user's plan (the `studio-bridge` zenoh-study and the `zenoh` /
  `zenoh_impl` branches). The Zenoh **storage plugin** is a Zenoh feature; it
  works the same whether the publisher is a Copper bridge or a plain `zenoh`
  publisher. Use `zenoh` directly.
- **ROS 2 as CDR-over-Zenoh.** Copper's ROS 2 support is just "speak DDS-CDR over
  Zenoh." That pattern (cf. `zenoh-bridge-ros2dds` / `rmw_zenoh`) is usable from
  Arora directly, and would be the natural successor to the current `ros2_client`
  (direct DDS) path if Arora migrates to Zenoh — again, no Copper needed.
- **Multi-format wire codecs (bincode/JSON/CBOR) selectable per channel.** A good
  design idea to copy into the bridge connector if/when it needs format
  negotiation.

## Verdict for the communication plug-ins

Don't reuse Copper's bridge **system** — it is inseparable from the Copper
runtime and models typed edges, not the store-mirroring bridge Arora has. Do
reuse the **ingredients** it is built from (Zenoh transport, Zenoh storage
plugin, ROS 2-over-Zenoh, per-channel codecs) **directly via the `zenoh`
crate**, which is the path `studio-bridge` is already on.
