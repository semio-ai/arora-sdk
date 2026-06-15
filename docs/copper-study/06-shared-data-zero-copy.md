# 6. Closing the performance gap: static linking + zero-copy shared data

> Sub-study of [README.md](README.md). Question: *Copper is much faster between
> modules. To match it in Arora, link statically with the behavior trees and the
> data storage, and let modules share objects across the boundary — specifically
> a data object that is a slice of the shared store, directly accessible through
> the module boundary. Is that technically feasible in a cross-language setup?*

## TL;DR

**Yes — and your instinct is right.** Copper is fast *because* its modules are
statically linked into one binary sharing one address space, exchanging
pre-allocated typed slots with no sandbox and no serialisation
([02](02-dynamic-modules.md), [01](01-static-core.md)). Arora's overhead is
exactly the two things that buys: the **sandbox boundary** (wasm) and the
**per-call serialisation**. Both are attackable:

| Executor | Can a store slice be shared zero-copy across the boundary? | Mechanism | Measured |
|----------|------------------------------------------------------------|-----------|----------|
| **Native** (`cdylib`, libloading) | **Yes, fully** — same address space, like Copper | pass `*mut DataSlice` (stable C layout) | ~**3.7 ns**/call, mutated in place |
| **WASM** (wasmtime) / **Browser** (js WebAssembly) | **Yes, across modules** (single-threaded) | one linear memory **imported into every guest** → all address the same bytes | shared read **verified** (`cross_module_shared_ok=true`) |
| any, when a copy is unavoidable | n/a | raw `memcpy` the slice instead of serialising | **7.7 ns** vs **40.9 ns** bincode (64 B) — ~5x |

All three are from [`sandbox/src/shared_data.rs`](sandbox/src/shared_data.rs):

```text
RESULT native_zero_copy_ns=3.71 cross_module_shared_ok=true wasm_memcpy64_ns=7.67 bincode64_ns=40.93
```

## Why Copper is faster, stated precisely

Nothing magic: Copper compiles all tasks into one process and wires them with a
`CopperList` of pre-allocated, typed slots. A "message" between tasks is a typed
memory write the next task reads directly — no crossing, no copy, no serde
([sandbox throughput](sandbox/src/throughput.rs): the whole 3-task chain is
~410 ns *including logging*; the hand-offs themselves are ~free). Arora pays, per
dynamic call, a host↔guest boundary (~59 ns for wasm) **plus** arora-buffers
ser/de of args and result (~2×28-40 ns) — see [02](02-dynamic-modules.md). To
approach Copper you remove those two terms where they are not earning their keep.

## Lever A — statically link the behavior-tree control flow

Today the BT's *control-flow logic* (sequence, fallback, …) lives in the
`behavior-tree-nodes` **wasm guest** and the host recurses into it via indirect
dispatch, so **every composite-node tick crosses the wasm boundary**
([`../dispatch.md`](../dispatch.md), [03](03-behavior-trees.md)). The tree
structure and blackboard are already host-side
([`arora-behavior-tree`](../../crates/arora-behavior-tree/readme.md)); only the
node semantics are in the guest.

Move that node logic into **statically-linked host Rust** and the per-node
crossing disappears entirely — composite traversal becomes ordinary Rust calls
(a few ns, like `bonsai-bt` in [03](03-behavior-trees.md)), while **leaves stay
dynamic module calls**. This is the single biggest BT win and is exactly "linking
statically with the behavior trees." (It also retires the unbounded
callable-registry growth flagged in [`../dispatch.md`](../dispatch.md) and
[issue #77](https://github.com/semio-ai/arora-engine/issues/77).)

## Lever B — a shared data slice across the boundary

This is the heart of the question. The answer differs by executor because the
*address space* differs.

### Native modules: a pointer into the store (true zero-copy)

A native `cdylib` loaded by `executor::native` runs **in the host address
space** — no sandbox. The host can hand it a raw pointer to a slice of the data
store and the module mutates it in place:

```rust
#[repr(C)] struct DataSlice { a: f64, b: f64, n: u32 } // layout pinned by arora-buffers
extern "C" fn module_process(p: *mut DataSlice) { unsafe { (*p).a += 1.0; } }
```

Measured ~3.7 ns/call with in-place mutation and **no copy at all**. This is
Copper's exact situation. Cross-language is already solved in-tree:
`arora-buffers` ships "Rust, C, C++ headers" for buffer layouts
([`../architecture.md`](../architecture.md)), so a C++ module sees the *same*
struct through a generated header. The only discipline needed is a stable
`#[repr(C)]` layout and an aliasing rule (below).

### WASM / browser modules: a shared *imported linear memory*

A wasm guest cannot hold a host pointer — its linear memory is isolated. The
naive reading is "so zero-copy is impossible." It isn't, with one move:
**create the data region as a linear memory and import it into every guest
module.** All instances then address the *same bytes*. Verified in the sandbox:
two separate instances import one `Memory`; module A writes `0xC0FFEE` at an
offset, module B reads the same offset back —
`cross_module_shared_ok=true`. The host reads/writes the same region via
`memory.data(&store)[off..off+len]` — a slice, no serde.

This works **single-threaded in both** wasmtime and the browser (import one
`WebAssembly.Memory` into each `Instance`). The store's hot region must *live in*
that shared linear memory rather than in a host-native `HashMap`, and a "slice
handle" becomes `(offset, len, type_id)` into it instead of a serialised blob.
Concurrent multi-threaded access additionally needs the wasm **threads** proposal
(`SharedArrayBuffer`, plus COOP/COEP headers in the browser) — but Arora's engine
loop is single-threaded today, so that is not on the critical path yet.

### When you still must copy: blit, don't serialise

If a particular value is owned host-side and a guest needs its own copy, a raw
`memory.write`/`read` of the bytes is ~**7.7 ns** for 64 B versus ~**40.9 ns** to
bincode it — ~5x, just by dropping the serialisation framing. So even the
no-sharing path has a cheap win: an arora-buffers "POD slice" mode that
`memcpy`s a `#[repr(C)]` view instead of running serde.

## Is it feasible cross-language? Yes, with three rules

1. **A pinned layout is the contract.** Every language addresses raw bytes, so a
   shared slice is a `#[repr(C)]`, fixed-offset, fixed-endian struct. This is
   precisely what `arora-buffers` already generates for Rust/C/C++; extend it
   with a "view/slice" kind (offset+len into a memory, no serialisation) and a
   `type_id` so the engine can check it. The UUID type system already gives you
   the identity ([`../dispatch.md`](../dispatch.md)).
2. **One owner of *where the bytes live*.** Native: the host store owns them,
   modules borrow a pointer. WASM: a single imported linear memory owns them,
   host and guests borrow offsets. Pick per-executor; the *handle* type
   (`Slice { mem, off, len, type }`) hides which.
3. **An aliasing/validity discipline.** Mutable shared slices need a rule:
   single-writer-per-tick, or epoch/generation counters, or a borrow token in
   the handle. Arora already lives with a deliberate unsafe sharing pattern
   (`engine as usize`, [`../design_decisions.md`](../design_decisions.md)); this
   is the same shape, scoped to data. Two wasm hazards to encode in the API:
   **memory growth invalidates host `data()` slices** (re-fetch after any grow),
   and **alignment** of offsets must match the `repr(C)` type.

## Suggested shape (sketch)

In `arora-types` (the interface layer the split already routes everything
through):

```rust
/// A typed window into the shared data store, valid for one access scope.
#[repr(C)]
pub struct Slice { pub ptr_or_offset: u64, pub len: u32, pub type_id: u128 }

pub trait SharedStore {
    /// Borrow a slice of the store. Native: ptr is a real address.
    /// Wasm: ptr_or_offset is an offset into the shared imported memory.
    fn view(&self, key: &Key) -> Option<Slice>;
    fn view_mut(&mut self, key: &Key) -> Option<Slice>;
}
```

- Native executor resolves `Slice` to a pointer and the module reads it directly.
- Wasm executor lays the store out in the imported memory and passes the offset;
  the generated guest stub reads it in place. No `Call`/`CallResult`
  serialisation for store access — that path is reserved for actual function
  invocation.

This is the concrete form of the "fixed data storage with a shared pointer
between modules" already floated in
[`../proposal-split-arora-repos.md`](../proposal-split-arora-repos.md) (Tiago's
note). The data here says it is worth doing — and feasible across all three
executors.

## Verdict

- **Static-link the BT node logic** → removes the per-node wasm crossing
  (biggest, easiest BT win).
- **Native modules** → share a `*mut` slice of the store directly; ~free,
  Copper-equivalent; cross-language via existing arora-buffers headers.
- **WASM/browser modules** → put the shared region in one imported linear memory;
  zero-copy across modules (verified), host reads slices with no serde; threads
  proposal only if you later need concurrency.
- **Everywhere** → a `repr(C)` "POD slice" path that `memcpy`s instead of
  serialising is a ~5x cheap win even without sharing.

You don't need Copper to get Copper-class inter-module performance — you need to
(a) stop crossing the sandbox for things that don't need isolation, and (b) stop
serialising things that can be addressed in place. Both are in-tree changes to
`arora-buffers` + the executors + `arora-behavior-tree`.
