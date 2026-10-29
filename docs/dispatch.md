# Dispatching calls

How the engine routes a call from its caller to the function that runs it:
the two ways a call names its target, what crosses the boundary, and the
entry point each kind of caller and module uses. The
[engine's readme](../crates/arora-engine/readme.md) introduces modules,
executors and calls.

## Two ways to name a target

A call is **direct** when it names a module function by its ids, and
**indirect** when it names a callable the host registered with the engine:

| | Direct | Indirect |
|---|---|---|
| Target | a module's function: module id and function id, two UUIDs | a registered callable: a `CallableId`, a `u64` |
| Arguments | an argument buffer, keyed by parameter id | none: the callable holds whatever it needs |
| Result | the return value and the mutated parameters | one `Value` |
| Lives | while the module is loaded | from `arora_register_callable` to `arora_unregister_callable` |
| Host entry point | `CallBridge::arora_call(Call)` | `CallBridge::arora_call_indirect(CallableId)` |
| Guest entry point | `host.dispatch(module, method, arg)` | `host.dispatch-indirect(callable)` |

A function id is a declared, stable identity shared by separately built
artifacts: both sides agree on it through the module's declaration. A
`CallableId` is a handle the engine draws at random when the host registers
a callable — an `Rc<dyn Callable>` — and means something only to that
engine, while the callable is registered. Only the host registers callables;
a guest can only call one whose id it was handed.

## Direct dispatch

A [`Call`](../crates/arora-types/src/call.rs) names the module and the
function, and carries the arguments as `StructureField`s whose ids are the
parameter ids. `Engine::arora_call` serializes them as an
[Arora Buffer](../crates/arora-buffers/readme.md): a structure whose id is the
function id and whose fields are the arguments (`call::serialize_to_arg`).
`Engine::dispatch` then:

1. finds the module by id, or fails with `ModuleNotFound`;
2. for a guest module, checks the arguments against the parameters its header
   declares
   ([details](../crates/arora-engine/readme.md#arguments-checked-against-the-header))
   and sets the call's deadline
   ([details](../crates/arora-engine/readme.md#time-and-memory-a-guest-may-spend));
3. hands the buffer to the module (`Module::dispatch`), which finds the
   function by id, or fails with `FunctionNotFound`:
   - a component, through its `dispatch` export;
   - a shared library, through its exported symbol `arora_function_<id>`;
   - a host module, through the closure attached under the id.

The function answers with a structure under the same function id: its first
field, under the function id too, holds the return value, and the others the
mutable parameters it wrote, under their parameter ids
(`call::encode_call_result`). `arora_call` reads it back as a
`CallResult { ret, mutated }`. A component's or a host module's error comes
back as `DispatchError::Guest`.

`CallBridge::arora_call_borrowed` takes the arguments by reference, for a
bridge that answers in-process without copying them; the engine builds the
owned `Call` from them.

## Indirect dispatch

`arora_call_indirect` looks the callable up in the engine's
[`CallableRegistry`](../crates/arora-engine/src/call.rs) and calls it with the
engine itself as its `CallBridge`, so the callable can make calls in turn. A
guest's `host.dispatch-indirect` returns the callable's `Value` serialized as
an Arora Buffer.

## Calls from a guest

A guest calls through the `arora:module` world's `host` interface, which every
host provides ([`wit/arora-module.wit`](../crates/arora-engine/wit/arora-module.wit)):

```wit
dispatch: func(module: id, method: id, arg: list<u8>) -> result<list<u8>, string>;
dispatch-indirect: func(callable: u64) -> result<list<u8>, string>;
```

The call re-enters the engine while the guest's own call is still under way —
the engine is borrowed further up the stack, which is why executors hold a raw
pointer to the pinned engine
([design decisions](design_decisions.md#raw-engine-pointer-in-executor-host-state-deliberately-unsafe)).
The callee runs under its caller's deadline, and its arguments are checked
as any guest's.

Generated bindings call through two functions over that interface, taking the
addresses of 16-byte UUIDs and of size-prefixed buffers in the guest's memory
and returning the address of the result buffer:

- `arora_dispatch(module_id, method_id, arg)` — in Rust generated from a
  `module.yaml` (`arora_module::__rt::engine`) and in C++
  ([`libs/cpp/src/arora/component.cpp`](../libs/cpp/src/arora/component.cpp));
- `arora_dispatch_indirect(callable_id)` — in Rust only.

The host's error comes back as an error buffer. In Rust the result buffer
stays valid until the guest's next call; in C++ the guest owns it.

## How behavior trees use indirect dispatch

[`arora-behavior-tree`](../crates/arora-behavior-tree/readme.md) registers
one callable per node of a tree when it lowers it, each holding its node and
its children's `TickId`s, and ticks the root with `arora_call_indirect`. A
node the library runs natively — a control node, a status leaf, a data node —
ticks its children through their callables without leaving the host. A node
backed by a module function is a direct call, `arora_call(Call { module_id, id,
args })`, whose arguments the tree reads from its variables.

A module's own control node receives its children as its first parameter,
named `children`, an array of `TickId`s, and ticks a child by calling
`arora_dispatch_indirect` with its callable id
([`modules/test-behavior-tree-nodes`](../modules/test-behavior-tree-nodes/readme.md)).
The guest holds no child's arguments — the tree bound them when it lowered —
so a handle saying "tick this child" is all it can give, and all it needs.

A tree replaced by another (`LoweredTree::unregister`) unregisters every
callable it registered.
