# 3. Behavior trees: Arora vs Copper

> Sub-study of [README.md](README.md). Question: *how about behavior trees in
> Copper, in general — are they competitive?*

## TL;DR

**Copper has no behavior tree at all**; "behavior trees in Copper" means bolting
on `bonsai-bt` — which doesn't need Copper. Bonsai is competitive as *tree
execution* and is far faster per tick (pure in-process Rust). But it loses
Arora's defining BT property: **leaves are calls into dynamically loaded,
cross-language modules**, and node types are themselves a loadable module. If you
need that, Copper+bonsai is not competitive; if you don't, you don't need Copper
to run a BT.

## Arora's behavior tree, precisely

From [`../dispatch.md`](../dispatch.md) and
[`../architecture.md`](../architecture.md):

- The **control-flow logic** (sequence, fallback, …) lives in the
  `behavior-tree-nodes` **wasm guest module**; the tree structure and blackboard
  live host-side in `arora-behavior-tree`.
- A tick recurses **host → guest → host**: the host ticks a node via
  `arora_call_indirect`, the guest control logic decides when to tick each child
  and calls back `arora_dispatch_indirect(child_id)`, re-entering the host.
- A **leaf** is an `arora_call` into a module function addressed by UUID — and
  that module can be wasm, native, C++ (`modules/polly`, `modules/nao`, …).
- Node primitives are typed records (`arora-behavior-tree-types`,
  `arora-behavior-tree-types-yaml`), so **trees and node sets are data**.

The consequences that matter for the comparison:

1. **Dynamic, cross-language leaves.** You add a new behaviour by *loading a
   module*, no recompile, in Rust/C++/wasm.
2. **Pluggable node vocabulary.** Even the composite nodes are a shipped module.
3. **A per-node cost.** Because composites run in a wasm guest and recurse via
   indirect dispatch, each node tick crosses the host↔guest boundary.

## Copper's side: nothing native, so `bonsai-bt`

The prior study searched copper-rs and found **zero** BT references; the closest
native concept is **missions** (compile-time DAG variants switched at runtime) —
mode-switching, not a behavior tree. So the realistic option is the leading Rust
BT crate, `bonsai-bt`, wrapped in a `CuTask`.

I built and ran a real bonsai tree
([`sandbox/src/bt_bonsai.rs`](sandbox/src/bt_bonsai.rs)):

```text
[battery-ok]   battery=80 last_action=Wave     final_position=150
[battery-low]  battery=5  last_action=GoCharge final_position=-1
[reloaded-from-json] battery=80 last_action=Wave final_position=150
RESULT json_len=183 pos_full=150 pos_low=-1 pos_reloaded=150 roundtrip_ok=true
```

Findings:

- **Semantics are competitive.** `bonsai-bt` 0.12 has `Sequence`, `Select`, `If`,
  `While`, `WhenAll/WhenAny`, `Wait`, `Invert`, etc., with `Success`/`Failure`/
  `Running` and a blackboard — the standard BT toolkit.
- **Trees are data, too.** With the `serde` feature, `Behavior<A>` serialises to
  JSON and reloads, and the reloaded tree behaves identically
  (`roundtrip_ok=true`). So *tree structure* is dynamic in bonsai as well — same
  as Arora's records.
- **But the leaf vocabulary is a compile-time enum.** The actions are a Rust
  `enum Act { BatteryAbove, MoveTo, Wave, GoCharge }`. Adding a new action type
  requires recompiling. There is **no** notion of a leaf that is a dynamically
  loaded, cross-language module call. This is the exact capability Arora's BT is
  built around.

## The performance trade is the mirror image of the flexibility trade

Using the measured primitives from
[`sandbox/src/wasm_dispatch.rs`](sandbox/src/wasm_dispatch.rs):

| | Per composite-node tick | Per leaf | Trees as data | Dynamic leaves | Cross-language |
|---|---|---|---|---|---|
| **Arora BT** | ~114 ns (guest control fn via `arora_call`) + ~59 ns per child callback | a module call (~114 ns wasm / ~28 ns native) | ✅ records | ✅ load a module | ✅ Rust/C++/wasm |
| **bonsai-bt** | a few ns (in-process recursion) | a few ns (enum match) | ✅ serde JSON | ❌ compile-time enum | ❌ Rust only |

So bonsai is **one to two orders of magnitude faster per tick** — it never leaves
the process — while Arora **trades that speed for dynamic, sandboxed,
cross-language behaviour**. Neither is strictly "better"; they optimise opposite
axes. (Note Arora can narrow the gap: its leaf cost drops to ~28 ns on the native
executor, and indirect dispatch already avoids per-call arg serialisation.)

## Does Copper add anything to the BT story? No.

`bonsai-bt` is a standalone crate. Running it inside a `CuTask` gains you only
Copper's scheduling/replay around the tick — irrelevant to whether the BT itself
is good, and paid for with Copper's whole-framework adoption. If Arora wanted a
*fast static BT*, it could depend on `bonsai-bt` directly in
`arora-behavior-tree` with **zero** Copper involvement. Copper's only native
contribution (missions) is strictly weaker than a behavior tree.

## Verdict for behavior trees

- If Arora keeps its current BT proposition — **dynamic, cross-language,
  module-backed leaves and a pluggable node set** — Copper/bonsai is **not
  competitive**, because bonsai's leaves are compile-time Rust.
- If a *static, Rust-only* BT is ever acceptable for a given product, `bonsai-bt`
  is a clean, fast choice — but adopt it **directly**, not via Copper.
- Copper itself brings no behavior tree to the table; "BTs in Copper" is a
  category error.
