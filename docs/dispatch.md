# Dispatching and indirect dispatch

How the engine routes a call from a caller to the function that runs it, the
two addressing schemes it supports today (direct and indirect), and how the
behavior-tree runtime leans on the indirect one.

For the buffer/ABI basics see [`crates/arora-engine/readme.md`](../crates/arora-engine/readme.md#call-mechanism);
for the *why* behind broader choices see [`design_decisions.md`](design_decisions.md).

## Two ways to address a call

The engine implements the [`CallBridge`](../crates/arora-types/src/call.rs) trait
(defined in `arora-types`, re-exported by the engine), which exposes two distinct
call paths. They differ in **what the target is addressed by** and **what is
passed**:

| | **Direct** | **Indirect** |
| --- | --- | --- |
| Host method | `arora_call(Call)` — module routed by `Call::module_id` | `arora_call_indirect(CallableId)` |
| Guest import | `arora_dispatch(module_id, method_id, arg)` | `arora_dispatch_indirect(callable_id)` |
| Target addressed by | `(module UUID, function UUID)` — u128 + u128 | `CallableId` — a `u64` |
| Arguments | an args buffer is passed | **none** — the target captured its state at registration |
| Returns | full call protocol: a `Structure` parsed into `CallResult { ret, mutated }` | a single serialized `Value` |
| Target lifetime | a permanent module export (lives while the module is loaded) | a runtime-registered callable (lives between register / unregister) |
| Resolution | two-level: `modules[module_id]` then `module.dispatch(function_id)` | one-level: `callables[callable_id]` |

Both are wired into each executor as host callbacks that capture the engine
pointer (`engine as usize`, see
[`design_decisions.md`](design_decisions.md#engine-as-usize-for-executor-callbacks-deliberately-unsafe)).
The wasmtime wiring is in
[`executor/wasm/mod.rs`](../crates/arora-engine/src/executor/wasm/mod.rs) (`func_wrap`
for `arora_dispatch` / `arora_dispatch_indirect`); the browser wiring is the
`env.arora_dispatch*` closures in
[`executor/browser/mod.rs`](../crates/arora-engine/src/executor/browser/mod.rs).

### Direct dispatch

`arora_dispatch(module_id, method_id, arg)`
([`wasm/mod.rs`](../crates/arora-engine/src/executor/wasm/mod.rs)) reads the two UUIDs
and the argument buffer out of guest memory, then calls
`Engine::dispatch(module_id, function_id, arg)`
([`engine.rs`](../crates/arora-engine/src/engine.rs)), which looks the module up in
`modules: HashMap<Uuid, Box<dyn Module>>` and forwards to `Module::dispatch`.
The native executor resolves the function by exported symbol name
(`arora_function_<uuid>`, [`executor/native.rs`](../crates/arora-engine/src/executor/native.rs));
the wasm executor resolves it through the module's `arora_functions` table.

The higher-level host wrapper `arora_call` adds the call **protocol** on top of
raw dispatch: it serializes the `Call` into the argument structure, and parses
the returned `Structure` into a `CallResult` whose first field is the return
value and whose remaining fields are mutated (out) parameters
([`engine.rs`](../crates/arora-engine/src/engine.rs)).

This is the "call a **named function** with these arguments" path. Both the
caller and the callee can be separately-compiled artifacts that agree only on
the UUIDs — the UUIDs are a published contract baked into `module.yaml` and the
generated stubs.

### Indirect dispatch

`arora_dispatch_indirect(callable_id)`
([`wasm/mod.rs`](../crates/arora-engine/src/executor/wasm/mod.rs)) takes a single
`u64`, calls `Engine::arora_call_indirect(CallableId)`, which looks the callable
up in the [`CallableRegistry`](../crates/arora-engine/src/call.rs) and invokes it. The
returned `Value` is serialized into a fresh buffer and handed back.

Callables are registered host-side with `arora_register_callable`, which mints a
**random** `u64` id (`rng.next_u64()`, [`call.rs`](../crates/arora-engine/src/call.rs))
and stores an `Rc<dyn Callable>`. `arora_register_callable` is **not** a guest
import — a guest cannot register a callable; only the process hosting the engine
can. `arora_dispatch_indirect` only lets a guest *invoke* a callable it was
handed a handle to.

This is the "invoke a **registered callable** by handle" path. The callable is a
closure that captured its state at registration time — which is exactly why
indirect dispatch passes no arguments.

## Direct vs indirect: name vs handle, definition vs instance

The two paths are *not* redundant, and the difference is not static-vs-dynamic
dispatch (both are runtime hash-map lookups). The real differences:

- **Name vs handle.** A function UUID is a *declared, stable identity* shared
  across separately-compiled artifacts. A `CallableId` is an *ephemeral handle*
  minted by the engine and handed back at runtime — meaningful only within one
  engine instance, only while registered.
- **Definition vs instance.** A module function UUID names one definition. A
  registered callable is an *instance* that has bound state. The behavior tree
  is the canonical case: many callables can be backed by the same underlying
  module function, each capturing different arguments and children (see below).
- **Argument binding.** Direct passes arguments per call. Indirect binds all
  state at registration and passes nothing.

> ⚠️ **Lifecycle caveat (current state).** As written, nothing in-tree calls
> `arora_unregister_callable` — there are no callers besides the trait/impl
> definitions. Registered callables therefore accumulate for the engine's
> lifetime. The behavior-tree runtime registers one callable **per node** at
> setup and never unregisters them, so repeatedly building trees grows the
> registry without bound. See the simplification issue referenced below.

## How behavior trees use indirect dispatch

The [`arora-behavior-tree`](../crates/arora-behavior-tree/readme.md) runtime is
a host-side orchestrator. The **basic control nodes** (sequence, fallback,
parallel, succeed, fail, run) are ticked **natively in the host** — they never
leave the process and never consult the engine. Only a node backed by a *module
function* — a leaf, or a module-provided composite — dispatches into the engine.
The tree structure and blackboard variables live host-side; **indirect dispatch**
is what lets a module node tick its children back in the host. (The
[`test-behavior-tree-nodes`](../modules/test-behavior-tree-nodes/readme.md) guest
module still carries wasm implementations of the basic nodes, but it is test-only
now — the runtime does not use it for control flow.)

### Setup: one callable per node

[`setup_tick_function`](../crates/arora-behavior-tree/readme.md) walks
the tree bottom-up. For every node it constructs a `TickFunction` capturing that
node, its already-registered children, and `Rc`-shared clones of the tree-wide
`function_index`, `locals`, and `node_arg_variables` maps. Each `TickFunction`
is registered via `arora_register_callable`, yielding a `CallableId` wrapped in
a `TickId` (defined in `arora-behavior-tree-types`).

Note that of the captured fields, only `node` and `children` are per-node — the
rest are `Rc` clones of the *same* tree-wide maps. The per-node `node`/`children`
are themselves recoverable from the tree's `node_index` (`node.children` already
holds child node UUIDs), which is why the per-node registration is largely
incidental rather than fundamental.

### Tick: host → guest → host recursion

```mermaid
sequenceDiagram
    participant Host as BT runtime (host)
    participant Eng as Engine
    participant Mod as module function

    Host->>Eng: arora_call_indirect(root TickId)
    Eng->>Host: invoke root TickFunction
    Note over Host: builtin composite ticks its children directly
    loop each child
        Host->>Eng: arora_call_indirect(child TickId)
        Eng->>Host: invoke child TickFunction
        alt builtin composite
            Note over Host: recurse in-process (no engine round-trip)
        else leaf or module-provided node
            Host->>Eng: arora_call(Call{module_id, id, args})
            Eng->>Mod: run the function
            Mod-->>Host: a module composite calls back arora_dispatch_indirect(grandchild)
        end
    end
    Host-->>Eng: composite Status
```

1. The host ticks the root with `arora_call_indirect(root TickId)`.
2. A **builtin composite** (sequence, fallback, parallel, …) ticks each child
   *directly* with `arora_call_indirect(child TickId)` — the control logic runs
   in-process, with no dispatch into a guest ([`behavior_tree.rs` `tick_builtin`](../crates/arora-behavior-tree/src/behavior_tree.rs)).
3. A **leaf** — or any node backed by a module function — dispatches into the
   engine with `arora_call(Call { module_id, id, args })`: one `Call` whose
   `module_id` routes it to the module.
4. Recursion: a child that is itself a builtin composite repeats step 2
   in-process; a **module-provided composite** receives its children as its first
   parameter (named `children`, UUID `5b6e9515-dbcc-411d-bee9-3d8cba5fedda`,
   typed `Vec<TickId>`) and calls back `arora_dispatch_indirect(child.callable_id)`
   to re-enter the host and tick a child
   ([`modules/test-behavior-tree-nodes/src/lib.rs`](../modules/test-behavior-tree-nodes/src/lib.rs)).

The child's arguments are not passed by the guest — they live host-side in the
blackboard variable graph and were bound at setup. The guest only knows an
opaque handle and says "tick this child," which is precisely what indirect
dispatch provides. This is why direct dispatch cannot replace it as-is: a child
is a *configured node instance*, not a named module function, and the guest does
not hold its arguments.

## Related

- A proposal to make indirect dispatch obsolete for behavior trees (by exposing
  a single `tick(tree_id, node_id)` registered through a unified registration
  path) is tracked in
  [semio-ai/arora-engine#77](https://github.com/semio-ai/arora-engine/issues/77).
