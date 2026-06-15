# 2. Dynamic modules and the "useless overhead" hypothesis

> Sub-study of [README.md](README.md). Questions: *I predict there may be useless
> overhead from having everything loaded and parsed dynamically. For the more
> dynamic modules, may Copper help at all?*

## TL;DR

- **Copper cannot help the dynamic modules — it is the exact opposite design.**
  It has **no** dynamic loading: the task graph is fixed at compile time
  (verified). Its "dynamic" escape hatches (Python tasks, FFI, wasm-in-a-task)
  are strictly worse than Arora's purpose-built loader and break the very
  guarantees Copper exists for.
- **The overhead is real but modest, and mostly mis-located.** Loading + parsing
  is a **one-time** cost at `load_module`, not per tick. The per-call cost is
  dispatch: measured at ~**114 ns** for a wasm module call vs ~**1.3 ns** for a
  static Rust call (~80-90x). On a native (`cdylib`) module the per-call cost is
  far lower (~the buffer ser/de, ~**28 ns**). Whether any of this is "useless"
  is purely a function of **call rate**.

## Copper has no dynamic loading — confirmed

The `#[copper_runtime(config = "copperconfig.ron")]` macro reads the RON graph
**at build time** and monomorphises the runtime. In the sandbox the macro
panics if `build.rs` does not pre-create its compile-time log index
([`sandbox/build.rs`](sandbox/build.rs)) — proof the work happens during
compilation, not at runtime. Changing the set of tasks or connections means
editing the RON and **recompiling**. There is no `load_module`, no `dlopen`, no
runtime type resolution — by design (it is what enables the zero-alloc,
pre-allocated `CopperList`).

This is precisely the capability that is Arora's reason to exist
([`../architecture.md`](../architecture.md): "the engine library… loads modules";
[`../dispatch.md`](../dispatch.md): UUID-addressed `arora_call`). **So Copper is
not a candidate to *be* the dynamic layer.** If anything, Arora is the thing
Copper lacks.

### The workarounds are worse than what Arora already has

The prior studio-bridge study catalogued them; the verdict for Arora is sharper:

| Approach | What it is | Why it loses to Arora's engine |
|----------|-----------|--------------------------------|
| Missions | pre-compiled DAG variants switched at runtime | still compile-time; can't add a behaviour you didn't ship |
| Python tasks | a task runs a Python script | no hot-swap; gives up Copper's latency/alloc guarantees |
| FFI in a task (`libloading`) | a task `dlopen`s `.so`s | unsafe, no cross-boundary typing, breaks zero-alloc + replay |
| wasm-in-a-task (`wasmtime`) | a task hosts a wasm runtime | **this is literally Arora's wasm executor**, reinvented inside one Copper task, minus Arora's UUID type system |

The last row is the punchline: to get Arora's dynamic modules under Copper you
would embed Arora's own mechanism inside a Copper task and lose Copper's
guarantees for that task. Net negative.

## Quantifying the overhead the user worries about

Hypothesis: "everything loaded and parsed dynamically" is wasteful. Splitting
that into its real components ([`sandbox/src/wasm_dispatch.rs`](sandbox/src/wasm_dispatch.rs)):

```text
direct_rust_ns        = 1.26    # static call baseline
wasm_guest_call_ns    = 20.91   # host -> guest exported fn (no callback)
wasm_host_roundtrip_ns= 58.55   # host -> guest -> host import + read arg buffer
bincode_roundtrip_ns  = 27.71   # serialise + deserialise a 16-byte payload
MODEL arora_dynamic_call_min_ns ~= roundtrip + 2*bincode = 114.0
```

### Where the cost actually is

1. **Parsing/loading is one-time, not per-call.** `load_module` parses the
   `module.yaml` header and instantiates the executor *once*. It does not recur
   per tick. So "parsed dynamically" is an amortised startup cost, essentially
   irrelevant to steady-state CPU. The user's instinct points at the wrong line.
2. **The per-call cost is the boundary + buffer.** For a **wasm** module that is
   the host↔guest crossing (~59 ns round-trip) plus arora-buffers ser/de of args
   and result (modelled by 2x bincode ≈ 55 ns) → ~**114 ns**. Arora's
   [`call.rs`](../../crates/arora/src/call.rs)/[`engine.rs`](../../crates/arora/src/engine.rs)
   do exactly this: serialise the `Call` into an args buffer, dispatch across the
   executor, parse the returned `Structure` into `CallResult`, plus two
   `HashMap<Uuid>` lookups.
3. **Executor choice dominates.** A **native** `cdylib` module (the `polly`/`nao`
   path, `executor::native`) has no wasm boundary — the call is a resolved symbol
   (looked up once at load) invoked directly, so the per-call cost collapses to
   roughly the buffer ser/de (~**28 ns**) plus a dynamic call. The wasm sandbox
   is what costs ~4x more.

### So is it "useless overhead"?

Only relative to call rate:

| Call site | Rate | Dynamic-call budget (wasm, 114 ns) | Verdict |
|-----------|------|-------------------------------------|---------|
| Behavior-tree leaves / module fns | ~10-100 Hz, tens of calls/tick | ~0.1 ms/s → <0.1% CPU | **negligible — leave it dynamic** |
| HAL sensor fan-out per joint | ~100-1000 Hz | sub-ms | negligible at native; small at wasm |
| Inner servo / control loop | 1-40 kHz, tight | could reach single-digit % | **don't put dynamic modules here** |

The architecture already respects this: the proposals keep the **HAL, data layer
and bridge static** (linked Rust) and reserve dynamics for **modules and
behaviour orchestration**, which run at behaviour rates. The overhead lands
where it is cheapest to afford.

## Levers that actually reduce the overhead (none of them are Copper)

If a profile ever shows dispatch as hot, the cheap wins are inside Arora:

- Prefer the **native executor** for hot modules (kills the wasm boundary).
- Cut buffer copies: a **shared fixed buffer / shared pointer between modules**
  is explicitly floated in [`../proposal-split-arora-repos.md`](../proposal-split-arora-repos.md)
  (Tiago's note) — that attacks the ser/de term directly.
- Use **indirect dispatch** where args are bound once
  ([`../dispatch.md`](../dispatch.md)): it passes no argument buffer per call, so
  it skips the args-serialisation term (the BT already uses this).

## Verdict for the dynamic layer

Copper offers the dynamic modules **nothing** and cannot host them without
becoming Arora. The overhead the user predicted is real, one-time-dominated at
load, ~0.1 µs per wasm call at steady state, and already confined to
behaviour-rate call sites where it is immaterial. Keep Arora's engine; if numbers
ever bite, the fixes are native-executor selection and shared buffers — not a
framework swap.
