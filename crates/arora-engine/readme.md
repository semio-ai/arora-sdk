# arora-engine

The engine loads **modules** — compiled code exporting functions — and calls
their functions by id, whatever runs them: a WebAssembly component on wasmtime
or in the browser, a native shared library, or plain Rust registered by the
host. Callers, and modules calling each other, never know which.

## Modules, executors and calls

A **module** is identified by a UUID and exports **functions**, each with its
own UUID and its parameters identified by UUID. What the engine loads is a
[`ModuleDefinition`](../arora-types/src/module/low.rs): the module's **header**
(its id, its name, the executor that runs it, its exported functions and their
parameters) and its **executable**, the bytes of its artifact. A header is
written from the module's declaration — a Rust
[`#[module]`](../arora-module/readme.md) or a `module.yaml` — when the artifact
is built.

An **executor** turns an executable into a running module. The header names
it (`executor.name`), and the engine holds one executor per name, added with
`EngineBuilder::add_executor`.

A **call** names a module id and a function id and carries its arguments as
[`Value`](../arora-types/readme.md)s keyed by parameter id
([`Call`](../arora-types/src/call.rs)). Its result is the return value and
the mutable parameters the function wrote
([`CallResult`](../arora-types/src/call.rs)). The engine implements
[`CallBridge`](../arora-types/src/call.rs), the trait callers program against;
[`docs/dispatch.md`](../../docs/dispatch.md) follows a call from caller to
function.

## Load a module and call it

The example C++ module ([`examples/modules/cpp`](../../examples/modules/cpp/readme.md))
adds two `i32`s:

```rust
use arora_engine::{call::CallBridge, engine::EngineBuilder, load::load_module_from_parts};
use arora_engine::executor::component::ComponentExecutor;
use arora_types::{call::Call, module::low::Header, value::{StructureField, Value}};
use uuid::uuid;

let mut engine = EngineBuilder::new()
    .add_executor(ComponentExecutor::new()?)
    .build();

let dir = "target/debug/modules/example-cpp";
let header: Header = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/header.json"))?)?;
let wasm = std::fs::read(format!("{dir}/example-cpp.wasm"))?.into_boxed_slice();
let module = load_module_from_parts(&mut engine, header, wasm)?;

let arg = |id, value| StructureField { id, value: Box::new(value) };
let result = engine.arora_call(Call {
    module_id: Some(module.id),
    id: uuid!("4a3020dc-6db3-43c2-bb75-6a7db237329a"), // add_two_integers
    args: vec![
        arg(uuid!("e15b0627-29df-441c-bea8-706ddc92f712"), Value::I32(2)), // a
        arg(uuid!("0374610f-5f4a-41d8-97f3-64f48cf21f40"), Value::I32(3)), // b
    ],
})?;
assert_eq!(result.ret, Value::I64(5));
```

[`load_module_from_parts`](src/load.rs) loads a header and its executable and
returns the module's id and its function ids, for a caller indexing functions
by module; `Engine::load_module` takes the `ModuleDefinition` itself. Loading
an id already loaded leaves the loaded module in place and succeeds.

A device built with the [`arora`](../arora/readme.md) crate does all of this
from its module directories.

## Executors

| `executor.name` | Executor | Runs | Built with |
|---|---|---|---|
| `wasm-component` | [`component::ComponentExecutor`](src/executor/component/mod.rs) | a `wasm32-wasip2` component, on wasmtime | the `wasmtime-host` feature |
| `wasm-component` | [`browser::BrowserComponentExecutor`](src/executor/browser/component.rs) | the same component, on the browser's `WebAssembly` | `target_arch = "wasm32"` |
| `native` | [`native::NativeExecutor`](src/executor/native.rs) | a shared library built for the host (`.so`, `.dylib`, `.dll`) | the `native-host` feature |

A WebAssembly guest is a component of the `arora:module` world
([`wit/arora-module.wit`](wit/arora-module.wit)), whatever language built it:

- it exports `functions`, the ids of the functions it serves, and `dispatch`,
  which runs one of them on an argument buffer and returns a result buffer;
- it may import the world's `host` interface, to call other modules, and the
  WASI 0.2 interfaces `wasi:cli`, `wasi:io`, `wasi:clocks`, `wasi:random` and
  `wasi:filesystem` with no directory preopened — no sockets
  ([`component_layout::HOST_INTERFACES`](src/component_layout.rs)); a
  component importing anything else does not load;
- loading it checks that it serves every function its header declares;
- a guest exporting `_initialize` — a C or C++ guest linked as a WASI reactor —
  has it run once, when it loads, before any call;
- a guest's error is the `err` of `dispatch`, or an error buffer in its `ok`;
- a component that traps (a Rust panic, a failed C++ `assert`) is entered no
  more: every later call fails with that trap until the module is reloaded.

A header naming `wasm` — a `wasm32-wasip1` core module — has no executor.
Rust guests become components through
[`arora_module::artifact!`](../arora-module/readme.md#build-the-artifact), C++
guests by linking `arora-cpp-component` ([`libs/cpp`](../../libs/cpp/CMakeLists.txt)).

In the browser the executor instantiates the component's guest core module
alone ([`component_layout::guest_module`](src/component_layout.rs)) and
implements its imports as a tab allows: standard output and error go to the
console, clocks read `performance.now()` and `Date.now()`, randomness comes
from `crypto.getRandomValues`, and nothing blocks. Chrome refuses to compile
more than 8 MB synchronously on the main thread: a
[`SharedLoader`](src/executor/browser/mod.rs) compiles a larger component
asynchronously (`SharedLoader::prepare`) and stages it for the next load of
that module id.

A native module is code in the host's process: the executor looks up
`arora_function_<id>` (the id's dashes as underscores) on each call, passes it
the address of the argument buffer and takes ownership of the result buffer it
returns. Nothing bounds its time or memory, and it is never unloaded: a native
library cannot be closed safely while the process runs.

## Host modules

Host code joins the same dispatch as a [`HostModule`](src/module.rs),
registered under its id with `Engine::register_module`. A module declared in
Rust registers whole:

```rust
engine.register_module(polly::ids::MODULE, Box::new(HostModule::of::<polly::Module>()));
```

Functions with no declaration attach to a [`ModuleBuilder`](src/module.rs):

```rust
let module = ModuleBuilder::new(MODULE)
    .function(DOUBLE, |call: Call| {
        let Value::I32(n) = call.args[0].value.as_ref() else {
            return Err(CallError::Guest { message: "expected an i32".into() });
        };
        Ok(CallResult { ret: Value::I32(n * 2), mutated: Vec::new() })
    })
    .build();
engine.register_module(module.id(), Box::new(module));
```

`ModuleBuilder::described_function` attaches a function with its name and
frozen signature, so a device lists it among its methods
(`HostModule::descriptions`); one attached with `function` is called but not
listed. `HostModule::from_exports` serves a contract's functions under the id
of the module implementing it (see
[contracts](../arora-module/readme.md#contracts-several-modules-one-set-of-functions)).

## Compiling once, reloading, unloading

A [`CompiledModule`](src/compiled.rs) holds a guest compiled once: each engine
loading it with `Engine::load_compiled_module` creates its own instance —
memory and globals — and compiles nothing. Natively every component runs on
one process-wide wasmtime engine, and a `CompiledModule` is `Send + Sync`; in
the browser it holds the guest core module's `WebAssembly.Module`.
`Engine::load_module` compiles first, then loads the compiled module.

`Engine::reload_module` replaces a loaded module with a fresh instance of a
`CompiledModule` under the same id; a module that fails to load leaves the old
one serving. `Engine::unload_module` drops a module, a registered host module
included. Neither is reachable from inside a call. A module whose executor
cannot unload it (`Executor::can_unload`, false for `native`) is refused with
`Unsupported` and stays loaded.

## Arguments checked against the header

Before a call reaches a guest, the engine checks its arguments against the
parameters the header declares for the function
([`arguments`](src/arguments.rs)): an argument naming a parameter the function
does not declare, or carrying a value not of its parameter's type, fails with
`DispatchError::Argument` and the guest never reads it. Calls between guests
pass the same check. It reads type tags in place and decodes nothing. What
passes:

- `unit` for any parameter — what callers send for an unset argument;
- any value for a parameter of a dynamic type (`KEY_VALUE_ID`,
  `ARRAY_VALUE_ID`);
- a present optional argument as an option or as its bare element;
- an array as its typed array, or as a generic array whose every element is
  of the element type.

Left to the guest: a buffer that is not a structure, a parameter left out, and
a record's fields — a record-typed argument is checked by its record's id, all
a header names of it. A host module's calls are not checked: it has no header.

## Time and memory a guest may spend

On wasmtime, a call to a guest ends by a deadline and its memory is capped
([`limits`](src/limits.rs)):

- **Time.** `Engine::set_step_budget` gives the time left in a device's step,
  which the guest calls of that step share. Outside a step a call gets
  `DEFAULT_CALL_BUDGET`, one second, and so does a module loading. A module's
  own `GuestLimits::call_budget` (`Engine::set_guest_limits`, or
  `EngineBuilder::with_guest_limits` for every module) tightens it. A call to
  another guest ends by its caller's deadline. A guest still running at its
  deadline is interrupted at the next epoch tick, every `EPOCH` (1 ms): the
  call fails with `DispatchError::Trap`, and the module refuses its calls
  until reloaded, since the call may have left its state half updated. A call
  with no time left is not made, and its module keeps serving.
- **Memory.** Each linear memory of an instance holds at most
  `GuestLimits::memory` bytes, `DEFAULT_MEMORY` (256 MiB) by default: a
  `memory.grow` past it returns -1 to the guest, whose allocator reports the
  failure.

In the browser a guest runs on the JS thread, which nothing interrupts: no
call time is bounded, and memory only by the maximum the module declares.

## Reference

**Features.** `wasmtime-host` (wasmtime and wasmtime-wasi) and `native-host`
(libloading), both default. A `wasm32` build sets `default-features = false`
and gets the browser executor.

**Errors.** Loading fails with `engine::LoadModuleError` (`ExecutorNotFound`,
`MalformedExecutable`, `Internal`). A dispatch fails with
`module::DispatchError`, which `arora_call` reports as `CallError`:

| `DispatchError` | `CallError` | When |
|---|---|---|
| `ModuleNotFound` | `ModuleNotFound` | no module under the id |
| `FunctionNotFound` | `FunctionNotFound` | the module serves no such function |
| `Argument` | `Generic` | an argument does not match the header |
| `Trap` | `Trap` | the guest trapped, overran its deadline, or trapped earlier |
| `Guest` | `Guest` | a component's or a host module's function returned an error |
| `Internal` | `Internal` | the engine or the executor failed |

**Threads.** An engine runs on one thread: guests call back into it while it
is borrowed further up the stack, through a raw pointer to the pinned engine
`EngineBuilder::build` returns (`Pin<Box<Engine>>`) — see
[design decisions](../../docs/design_decisions.md#raw-engine-pointer-in-executor-host-state-deliberately-unsafe).
A `CompiledModule` crosses threads.

**Header fields read.** The engine reads a header's `id`, `executor.name`,
`exports`, and `name` in error messages. `executor.min_version`,
`executor.max_version`, `imports` and `executable_mime` describe the module to
tools and registries; the engine does not check them.
