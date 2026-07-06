# Dynamic Module Loading and Hot-Swapping

> Sub-study of [copper_study.md](copper_study.md) — §4.6 "Dynamic loading".

## Question

Is there any chance that I can have a similar functionality as the Arora Engine from
the `engine` repo, like dynamic loading of modules, and hot-swapping them? Even if
they do not do any data subscription?

---

## Findings

### 1. Copper Does Not Support Dynamic Loading — by Design

Copper's architecture is **compile-time**. The `#[copper_runtime(config = "copperconfig.ron")]`
proc macro reads the task graph at compile time and generates a monomorphized runtime
where all task types, message types, and connections are known statically.

**Grounding** — from the wiki:

> *"The config is read at compile time by a proc macro to generate what we call
> CopperLists. Those are pre-allocated, zero-alloc, cache-friendly execution buffers."*

This means:
- **No dynamic dispatch**: Each task slot in the CopperList is a concrete type
- **No runtime type resolution**: Message types are resolved at compile time
- **No plugin loading**: No `dlopen`, no WASM module loading, no `libloading`

**Searched copper-rs for**: "dynamic", "hot-swap", "hot_reload", "dlopen", "libloading",
"plugin". **Zero results** for any dynamic loading mechanism.

### 2. What the Former Arora Engine Could Do

The former Arora engine (from `semio-ai/engine`, private) supported dynamic module loading
through its `arora-types` module system:

```yaml
# Module definition (serializable)
header:
  id: "550e8400-..."
  executor:
    name: "WebAssembly"    # Runtime selection
  exports:
    - { name: "tick", ... }
  imports:
    - { name: "get_sensor", module_name: "sensor-module", ... }
executable: <WASM bytes>   # The actual module code
```

**Capabilities**:
- Load WASM modules at runtime
- Load native dynamic libraries
- Resolve imports/exports between modules dynamically
- Hot-swap: replace a module while the engine runs

### 3. Can This Be Replicated with Copper?

**Short answer: No, not natively. But there are workarounds.**

#### Option A: Python Tasks (for prototyping)

Copper supports Python tasks (`cu_python_task`) that run Python code alongside Rust tasks.
While not true dynamic loading, it enables:
- Changing behavior without recompilation
- Loading Python scripts at runtime
- Two execution modes: `process` (separate interpreter) or `embedded` (PyO3)

**Grounding** — from `examples/cu_python_task_demo/README.md`:

```markdown
# cu_python_task_demo
Shows how Copper can run one task from Python while the rest stays in Rust.

The Python Contract:
def process(ctx, inp, state, output): ...
def start(ctx, state): ...   # optional
def stop(ctx, state): ...    # optional

This feature is for rapid experimentation only. Python on Copper's execution path
gives up low latency, low jitter, tight allocation control.
```

**Limitation**: No hot-swap. The Python script is loaded at task creation time. Changing it
requires restarting the task (or the entire Copper app).

#### Option B: Missions for Mode Switching

Copper's missions system allows switching between **pre-compiled** DAG configurations.
This isn't dynamic loading, but it covers the use case of switching behavioral modes:

```rust
// At runtime, switch from mission A to mission B
app_a.stop_all_tasks()?;
drop(app_a);

let mut app_b = MissionBApp::builder()
    .with_clock(clock.clone())
    .with_log_path(&logger_path, SLAB_SIZE)?
    .build()?;
app_b.start_all_tasks()?;
```

**Limitation**: All missions must be known at compile time. You cannot define new missions
at runtime.

#### Option C: Modular Configuration with Includes

Copper supports modular RON configs with parameter substitution:

```ron
// motors.ron (template)
(
    tasks: [
        (id: "motor_{{id}}", type: "tasks::Motor", config: {"pin": {{pin}}}),
    ],
)

// main_config.ron
(
    includes: [
        (path: "base.ron"),
        (path: "motors.ron", params: {"id": "left", "pin": 5}),
        (path: "motors.ron", params: {"id": "right", "pin": 6}),
    ],
)
```

**Grounding** — from `examples/modular_config_example/README.md`:

```markdown
Demonstrates Copper's modular configuration with includes and parameter substitution.
Benefits: Reusability, maintainability, parameterization, organization.
```

**Limitation**: Still compile-time. Parameters are resolved by the proc macro at build time.

#### Option D: Hybrid Architecture (Custom Dynamic Layer)

For true dynamic loading, you could build a **thin dynamic dispatch layer** inside a
single Copper task:

```rust
use libloading::{Library, Symbol};

#[derive(Reflect)]
pub struct DynamicModuleTask {
    libraries: Vec<Library>,
    current_module: usize,
    module_dir: String,
}

impl Freezable for DynamicModuleTask {}

impl CuTask for DynamicModuleTask {
    type Input = input_msg!(ModuleInput);
    type Output = output_msg!(ModuleOutput);

    fn new(config: Option<&ComponentConfig>) -> CuResult<Self> {
        let module_dir = config.unwrap().get::<String>("module_dir").unwrap();
        // Load all .so/.dylib files from the directory
        let libraries = Self::load_modules_from_dir(&module_dir)?;
        Ok(Self { libraries, current_module: 0, module_dir })
    }

    fn process(&mut self, _ctx: &CuContext, input: &Self::Input, output: &mut Self::Output) -> CuResult<()> {
        // Check for hot-swap signal
        if self.should_reload() {
            self.libraries = Self::load_modules_from_dir(&self.module_dir)?;
        }

        // Call the current module's process function via FFI
        unsafe {
            let lib = &self.libraries[self.current_module];
            let process_fn: Symbol<fn(&[u8]) -> Vec<u8>> = lib.get(b"process")?;
            let input_bytes = bincode::encode(&input.payload())?;
            let output_bytes = process_fn(&input_bytes);
            output.set_payload(bincode::decode(&output_bytes)?);
        }
        Ok(())
    }
}
```

**Tradeoffs**:
- ✅ True dynamic loading at runtime
- ✅ Hot-swapping possible (reload `.so` files)
- ❌ Unsafe FFI boundary
- ❌ No compile-time type checking across the boundary
- ❌ Serialization overhead at the FFI boundary
- ❌ Breaks Copper's zero-alloc guarantee for this task
- ❌ Breaks deterministic replay (loaded module may differ between runs)

#### Option E: WASM Modules in a Task

Similar to Option D but using WASM for sandboxing:

```rust
use wasmtime::{Engine, Module, Store, Instance};

#[derive(Reflect)]
pub struct WasmModuleTask {
    engine: Engine,
    store: Store<()>,
    instance: Option<Instance>,
    wasm_path: String,
}

impl CuTask for WasmModuleTask {
    // ... load and run WASM modules dynamically
    // Similar to the former Arora engine's WASM executor
}
```

**Tradeoffs**:
- ✅ Sandboxed execution (memory safety, no undefined behavior)
- ✅ Cross-platform (WASM modules are portable)
- ✅ Closer to the former Arora engine's model
- ❌ WASM runtime overhead (~10-100x slower than native for compute)
- ❌ Limited WASM system access (no direct hardware I/O)
- ❌ Breaks zero-alloc, breaks deterministic replay

### 4. Comparison: Dynamic Loading Approaches

| Approach | Dynamic? | Hot-swap? | Type-safe? | Zero-alloc? | Replay? |
|----------|----------|-----------|-----------|-------------|---------|
| **Native Copper** | ❌ | ❌ | ✅ | ✅ | ✅ |
| **Missions** | Compile-time | Stop/start | ✅ | ✅ | ✅ |
| **Python tasks** | At startup | ❌ | ❌ | ❌ | ❌ |
| **FFI (libloading)** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **WASM (wasmtime)** | ✅ | ✅ | Partial | ❌ | ❌ |
| **Former Arora** | ✅ | ✅ | ✅ (UUID types) | ❌ | ❌ |

### 5. Recommendation for Arora

If dynamic module loading is a **hard requirement**, Copper alone won't satisfy it.
The options are:

1. **Accept compile-time composition**: Use Copper missions for behavioral mode switching.
   This covers most robotic use cases (patrol mode, interaction mode, charging mode).

2. **Hybrid**: Use Copper for the real-time pipeline and keep a separate dynamic module
   system (like the former Arora engine) for higher-level behavior orchestration.
   The BT task (Option D/E above) bridges the two worlds.

3. **Don't use Copper for modules**: Keep the current `StudioBridgeController` trait pattern
   for the module interface, and use Copper only for the low-level control pipeline
   within each module.

---

## Summary

| Question | Answer |
|----------|--------|
| Does Copper support dynamic loading? | **No** — by design, everything is compile-time |
| Can modules be hot-swapped? | **Not natively** — workarounds exist (FFI, WASM, Python) |
| How does this compare to the former engine? | The former engine had full dynamic loading; Copper trades this for zero-alloc determinism |
| What's the recommendation? | If dynamic loading is required, use a hybrid approach or accept Copper's missions system |
