# Copper-rs for Arora: a focused study

Status: study, for review.
Date: 2026-06-15.
Author: Victor (with research help from an LLM agent).

## Why this exists

Arora is a **dynamic module engine**: it loads modules (wasm / native / browser)
at runtime, identifies their types and functions by UUID, and dispatches calls
across the host↔guest boundary (see [`../architecture.md`](../architecture.md)
and [`../dispatch.md`](../dispatch.md)). The two split proposals
([`../proposal-split-arora-repos.md`](../proposal-split-arora-repos.md),
[`../proposal-bring-studio-bridge-in.md`](../proposal-bring-studio-bridge-in.md))
plan a **static core** underneath that dynamism:

- a **HAL** trait (`arora-sdk::Hal`),
- a **data layer** (`arora-types::DataStore`, impl `arora-ecbs`),
- a **communication bridge** (`studio-bridge` as a connector),
- and possibly **behavior-tree execution**.

[Copper](https://github.com/copper-project/copper-rs) (`cu29`) makes the opposite
bet from Arora: it **statically assembles** a task graph at compile time. The
question this study answers: *for the parts of Arora we want static anyway, does
Copper buy us anything — and could it help the dynamic parts, or replace a
redundant project?*

> A prior study exists in `studio-bridge/copper-study` (branch
> `copilot/study-copper-rs-documentation`). It is thorough but (a) framed from
> studio-bridge's simpler key-value remote-control angle, and (b) its "tests" are
> hand-written mocks that never compile or run Copper. This study is Arora-centric
> and every quantitative claim is backed by the runnable [`sandbox/`](sandbox/)
> crate against **Copper 1.0.0-rc2** (the prior study targeted 0.15).

## The findings in one screen

| # | Question | Short answer |
|---|----------|--------------|
| 1 | Would Copper help the **static core** (HAL, data layer, bridge)? | **Mostly no.** Copper replaces the *driver loop*, not these components, and its typed-per-edge data model fights Arora's shared-blackboard `DataStore`. It does not provide a HAL or a key-value store. See [01](01-static-core.md). |
| 2 | Can Copper help the **dynamic modules**, or is the "useless overhead" real? | **It cannot help; it is the opposite philosophy** (no dynamic loading — compile-time DAG, verified). The overhead is real but small: a dynamic wasm call ~**114 ns** vs ~**1.3 ns** static. Whether that is "useless" depends entirely on call rate. See [02](02-dynamic-modules.md). |
| 3 | Are Copper's **behavior trees** competitive? | **Copper has no behavior tree.** You bolt on `bonsai-bt` (works, verified). It is competitive as *tree execution* but loses Arora's defining feature: leaves that are **dynamically loaded module calls**, and trees that are **records**. See [03](03-behavior-trees.md). |
| 4 | Is the **communication plug-in system** worth reusing? | **Only if you adopt the Copper runtime.** `cu-zenoh-bridge` / `cu-ros2-bridge` exist (1.0.0-rc2) but are `CuBridge`s wired into a Copper task graph. Outside it, the raw `zenoh` crate is simpler. See [04](04-communication-plugins.md). |
| 6 | **How do I get Copper-class performance in Arora** (static BT, shared data slice across the boundary)? | **Feasible, and the right plan.** Native modules can share a store slice by pointer (~free); wasm/browser modules can share **one imported linear memory** zero-copy across modules (verified); static-linking the BT node logic removes the per-node sandbox crossing. See [06](06-shared-data-zero-copy.md). |
| 5 | All in all — switch, reuse, or avoid a redundant project? | **Do not adopt Copper as Arora's core.** The gut feeling ("at best I avoid a redundant project") does not hold: Copper would *be* the redundant project here, because its value (deterministic replay, zero-alloc DAG) does not target Arora's bottleneck (dynamic composition). Cherry-pick ideas, not the framework. See [05](05-verdict.md). |

## Sub-studies

1. [**Static core: HAL, data layer, bridge**](01-static-core.md)
2. [**Dynamic modules and the overhead hypothesis**](02-dynamic-modules.md)
3. [**Behavior trees: Arora vs Copper**](03-behavior-trees.md)
4. [**Communication plug-ins: Zenoh / ROS 2 bridges**](04-communication-plugins.md)
5. [**Verdict**](05-verdict.md)
6. [**Closing the performance gap: static linking + zero-copy shared data**](06-shared-data-zero-copy.md)

Plus the [**runnable sandbox**](sandbox/) and its [results](sandbox/README.md).

## How the claims are grounded

Every "Copper does X" claim is one of:

- **Run** — a binary or test in [`sandbox/`](sandbox/) with a captured `RESULT` line.
- **Source** — a path into `cu29 1.0.0-rc2` in the local cargo registry, or into
  this repo for the Arora side.
- **Doc** — Copper's published docs/README, cited inline.

Where docs were ambiguous (they often were — `cu29` ships "0% documented" on
docs.rs for several crates), the sandbox settles it by compiling and running.
