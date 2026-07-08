# Proposal — Arora API consistency & the behavior model

**Status:** design review / proposal for review. Motivated by the observation that the `arora` crate has accreted several overlapping setup paths and a few structural discrepancies, and that "predetermination" (nodes hard-wired to specific keys) is leaking into the behavior design. This reviews the *current* state with `file:line` evidence, states the *target* model, and proposes the changes — to be done **before** finishing the Vizij orchestrator migration, because the orchestrator collapses into a clean shape only once arora's model is clean.

All citations are `arora-sdk@origin/main` (`6b0973de`) unless marked `[vizij-rs]` or `[Step C branch]`.

---

## 1. The target model (what we want)

- **One `Arora` type, built with a builder**, with sensible defaults:
  - `.with_bridge(..)` — **several allowed** (fan-in on reads, fan-out on writes);
  - `.with_hal(..)` — exactly one;
  - `.with_data_store(..)` — accepts `Arc<dyn DataStore>`, not a concrete `SimpleDataStore`;
  - `.with_behavior_interpreter(..)` — the one interpreter;
  - `.with_module(..)` — **several allowed**.
- **`Arora::step(dt)`** reads top-to-bottom as: *time update → HAL readings → bridge readings → behavior tick → HAL writings → bridge writings.*
- **`Arora::run`** is literally the loop iterating `Arora::step`.
- **The behavior interpreter is a statically-known component** (not a module), but is **addressable from the bridge via an ordinary `AroraCall`** — its edit/load operations are **registered to the engine as statically-known functions**, under a **well-known ("golden") module reference** that `arora-behavior` exposes. No bypass of the engine.
- **One graph model** (in `arora-behavior`): a graph is *nodes bound to functions* (statically-known **or** module calls — the way `arora-behavior-tree` already treats both homogeneously), with **inputs/outputs** whose data types are **arora `Value` types**, and **links** between I/Os (or between nodes). Each behavior interpreter reads the links its own way; expressions may eventually appear in links.
- **Behavior edition** is expected *from the interpreter trait*: it accepts **graph diffs** (new nodes, new links).
- **Predetermined I/Os**: some nodes (typically animation) are authored with specific keys in mind. Predetermination hurts reusability but simplifies wiring — so we **allow** an I/O to carry a predetermined key, and an interpreter may **override it by linking** that I/O to another key. (Setting/overriding is done *with links*.)

---

## 2. Current state, with evidence

### 2.1 Construction & lifecycle — two types, six funnels, no builder

**`Arora` = engine + function index, nothing else.** `crates/arora/src/lib.rs:39-45`:
```rust
pub struct Arora { engine: PinnedEngine, function_index: Rc<HashMap<Uuid, ModuleFunction>> }
```
Its only constructor is `Arora::start()` — `async`, no args, builds the engine and an **empty** `function_index` (`lib.rs:51-57`). The index is documented to fill "when a runtime loads a real module" (`lib.rs:42-44`) **but no module-load API exists** — confirmed by the test comment at `runtime.rs:813-815` ("`Arora` exposes no module-load API"). So `.with_module(..)` is a missing seam and the index is always empty.

**`Runtime` = the loop**, a *separate* type (`runtime.rs:144-165`). Building a device always means `Arora::start()` → destructure it into `Runtime::with_io_in` (`runtime.rs:203-206`); `Arora` never survives. So the "bundle" is a two-field carrier between `start()` and `with_io_in`, and it additionally carries **two dead run methods** — `Arora::run_groot_xml` (`lib.rs:61-79`) and `Arora::run_forever` (`lib.rs:87-92`, a `loop { sleep }` placeholder) — that bypass the loop entirely (no store/bridge/HAL/step). A second, dead-end run path.

**`with_io` hard-wires `SimpleDataStore`; `with_io_in` takes the trait object.** `runtime.rs:179-185`:
```rust
pub fn with_io(arora, hal, bridge) -> (Self, impl Future) {
    Self::with_io_in(arora, hal, bridge, Arc::new(SimpleDataStore::new()))
}
```
`with_io_in(arora, hal, bridge, store: Arc<dyn DataStore>)` (`runtime.rs:197-232`) is the real constructor. So `with_io` is pure "peel-away-a-default" sugar.

**Single bridge, single HAL, a *queue* of behaviors.** The bridge and HAL live only inside the `io` pump (`runtime.rs:215`, `io(bridge, hal, …)`); behaviors are a `VecDeque<QueuedBehavior>` (`runtime.rs:152`) populated **after** construction via `queue_behavior` / `queue_named_behavior` / `queue_groot_xml` / `queue_named_groot_xml` (`runtime.rs:247-306`). STEP 2 pops the front, ticks it, and re-queues it if `Running` (`runtime.rs:352-373`) — so it is a **round-robin of behaviors**, not one well-defined current interpreter.

**Six overlapping `run*` funnels** (self-described "peel a default each", `run.rs:1-24`): `run()` → `run_with_hal()` (two cfg variants) → `run_with()` → `run_with_bridge_builder()` → `run_with_frontend()` (the real body, `run.rs:131-190`), plus `studio::run_with_hal` (`studio/mod.rs:47-131`). The public `run_with*` take a **concrete `SimpleDataStore`** (`run.rs:95,111,133`) and wrap it as `Arc<dyn DataStore>` at `run.rs:153` — inconsistent with `with_io_in`, which already takes the trait object. There is **no `arora::launch`** (grep: none).

**arora-web adds two more surfaces + a third `dt` unit.** `crates/arora-web/src/lib.rs`: `BrowserRuntime::start(hal, bridge, store: Arc<dyn DataStore>)` (`lib.rs:94-111`, the web analogue of `run_with_frontend`) and the `#[wasm_bindgen] AroraRuntime::start()` (`lib.rs:245-254`, hard-wiring `FakeHal`+`FakeBridge`+`SimpleDataStore`). Three `dt`/return conventions for one operation: `Runtime::step` takes **ns** → `StepOutcome` (`runtime.rs:319`); `BrowserRuntime::step` takes **ns** → `bool` (`lib.rs:136`); `AroraRuntime::step` takes **ms** → `bool` (`lib.rs:262`).

**`step()` sequence** (`runtime.rs:319-402`) already reads roughly as the target order — STEP 1a HAL drain (`320-323`), 1b bridge drain (`324-338`), 1c golden clock (`339-351`), 2 tick front behavior (`352-373`), 3 coalesce+flush outbound (`374-400`) — but the "1a/1b/1c/2/3" labels are ad-hoc comments, the outbound flush fans to both `bridge.send_data` and `hal.write` inside the io pump (`runtime.rs:537-547`), and there is no single named `time / hal-read / bridge-read / tick / hal-write / bridge-write` spine.

### 2.2 The call seam & bridge addressing

**A function is a `(module_id, function_id)` UUID pair.** `crates/arora-types/src/call.rs:9-53`: `Call { module_id: Option<Uuid>, id: Uuid, args }`, `CallResult { ret, mutated }`, `trait CallBridge { fn arora_call(&mut self, module: &Uuid, call: Call) -> Result<CallResult, CallError>; … }`. The engine implements it by UUID lookup: `Engine::dispatch` → `modules.get_mut(module_id)` → `Module::dispatch(function_id, arg)` (`arora-engine/src/engine.rs:135-147`, `module.rs:25`).

**`BridgeOp::Call` is an unimplemented stub.** `crates/arora-bridge/src/lib.rs:37-59` defines `Get / Update / Call(Call) / ListKeys / ListMethods`; `Runtime::handle_command` wires `Get`→read, `Update`→write, `ListKeys`/`ListMethods`→enumerate — but `runtime.rs:432-435`:
```rust
BridgeOp::Call(_call) => {
    // TODO(next slice): dispatch the call through the engine.
    Err("call handling is not yet wired".to_string())
}
```
So calling *any* function from the bridge is a pending TODO. This is convenient: wiring `BridgeOp::Call` → `Engine` dispatch is exactly where the interpreter's golden edit-functions (§1) plug in — no special case.

**The behavior interpreter is not addressable from the bridge at all today.** The only ways to load/swap a behavior are the **local** `Runtime::queue_*` methods, reached from native startup (`run.rs:173`) and the in-process wasm/JS bindings (`arora-web/src/lib.rs:119-126, 268-269`) — never from a `BridgeOp`. No well-known function id routes to the interpreter.

**The `BehaviorInterpreter` trait is `tick`-only — no edition.** `crates/arora-behavior/src/lib.rs:84-88`:
```rust
pub trait BehaviorInterpreter {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError>;
}
```
`BehaviorContext { store, caller }` (`lib.rs:48-53`). No `edit`/`load`/`apply_diff`. Every "load a behavior" path is a **full rebuild** then queue-a-new-interpreter (`runtime.rs:258-284`; the running `BehaviorTreeInterpreter` holds an immutable tree, `arora-behavior-tree/src/behavior.rs:18-42`). Accepting a graph diff is not expressible against the trait today.

### 2.3 The two graph models — one already homogeneous, one closed, none shared

**`arora-behavior-tree` already unifies static + module functions** — this is the model to generalize. A node is *just* a function id + typed argument expressions + optional children (`arora-behavior-tree/src/schema.rs:21-39`):
```rust
pub struct Node { id: Uuid, function: Uuid, arguments: HashMap<Uuid, Expression>, children: Option<Vec<Uuid>> }
```
`Expression` already models links: literal `Value`, `VariableId`, or `NodeArgument(NodeParameterId{node, parameter})` (`schema.rs:48-64`). Control nodes and module actions are the **same struct**; `tick()` routes by id — native for the seven builtins (`tick_builtin`, `behavior_tree.rs:158-268`, ids `SEQ_FUNCTION_ID` etc.), else `caller.arora_call(module_id, call)` into a frozen `Function` record (`behavior_tree.rs:288-435`, `407-409`). Groot XML is a string-name front-end onto the same model (`schema_groot.rs:42-118`). Typed I/O comes from the frozen `Function` record (`arora-types/src/record/module.rs:147-151`) whose params/return are `structure`/`enumeration` type records (`arora-types/src/record/{structure,enumeration}.rs`), and values are `arora_types::value::Value` (each carrying a type UUID, `value.rs:79-186`).

**Vizij's node graph is a different, closed representation** `[vizij-rs]`. `GraphSpec { nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec>, … }` (`vizij-graph-core/src/types.rs:310-324`); a node's function is a **closed `NodeType` enum of ~110 variants** (`types.rs:30-154`) with one hard-coded Rust handler each (`eval_node.rs:205-308`); ports are named strings; links are an out-of-line `edges` list with selectors (`types.rs:376-405`); values are vizij's **own** `Value`+`Shape` (`vizij-api-core/src/value.rs:48-96`), emitted through dedicated `Input`/`Output` sink nodes (`TypedPath`→`WriteOp`, `eval_node.rs:188-201`). The Step-C `ModuleCall` node reaches a runtime-resolved function, but through a *new* `GraphHost`/`CallTarget` seam invented for the graph `[Step C branch: vizij-graph-core/src/host.rs]` — so a module call is a *special node kind* in vizij, whereas in BT it is the ordinary node.

**No shared graph model exists.** BT (`TreeNode`/Groot) and vizij (`GraphSpec`) are disjoint types in disjoint crates; grep finds zero cross-references. Their only shared ground is `CallBridge`/`Call` (used natively by BT, reached by vizij only via the interop adapter `vizij-arora-behavior/src/host.rs`) and, via explicit conversion, the two distinct `Value` types.

**Predetermination already exists — twice.** The animation module's `step` emits `TrackOutput { track_id, default_key, value }` carrying each track's **authored key** as `default_key` `[Step C branch: interop/vizij-animation-module/src/lib.rs:108-125]`; and a vizij `Output` node carries an authored `params.path: TypedPath` as its predetermined sink `[vizij-rs: vizij-graph-core/src/types.rs:268-274]`. Both are "an I/O with a predetermined key," today overridable only by rewiring which value reaches the node.

---

## 3. Gap analysis → proposed changes

### 3.1 One `Arora` with a builder (folds `Runtime` in)

Fold the loop into `Arora`; delete the `Arora`/`Runtime` split, the `with_io`/`with_io_in` pair, the six `run*` funnels, and the two dead `Arora::run_*` methods. One construction expression:
```rust
let (arora, io) = Arora::builder()
    .with_data_store(store)          // Arc<dyn DataStore>; default: SimpleDataStore
    .with_hal(hal)                   // exactly one; default: FakeHal
    .with_bridge(ws_bridge)          // repeatable; default: none (or the local ws server)
    .with_bridge(loopback_bridge)    // several allowed
    .with_module(anim_module)        // repeatable; default: none
    .with_behavior_interpreter(bt)   // the one interpreter; default: an empty BT
    .build();                        // -> (Arora, impl Future /* the io pump */)
```
Defaults match today's `run()` (FakeHal, local ws bridge, empty BT, SimpleDataStore) so `Arora::builder().build()` ≈ `arora::run()` setup. `with_module` finally makes the advertised (but absent) module-load seam real, populating `function_index`. The behavior interpreter becomes a build-time injection, replacing the round-robin queue with one well-defined interpreter (edition, §3.4, replaces "queue another behavior").

### 3.2 `step(dt)` and `run()` as a legible spine

```rust
pub fn step(&mut self, dt: Duration) -> StepOutcome {
    self.update_time(dt);        // golden arora/time, arora/dt
    self.read_hal();             // drain HAL sensor updates -> store
    self.read_bridges();         // drain every bridge's inbound -> store / commands
    self.tick_behavior();        // the one interpreter ticks against the store
    self.write_hal();            // flush store changes -> HAL actuators
    self.write_bridges();        // flush store changes -> every bridge
    outcome
}
pub fn run(&mut self) { while self.step(self.measured_dt()) != Unregistered {} }
```
Six named phases, one per line, matching the target sentence. `run` is visibly the loop over `step`. One `dt` type (`Duration`) and one return (`StepOutcome`) across native and web; the web wrapper converts its rAF milliseconds at the boundary only.

### 3.3 Multiple bridges

Hold `bridges: Vec<Arc<dyn Bridge>>`. `read_bridges` drains all of them (fan-in) into the store/command handling; `write_bridges` sends each coalesced outbound `StateChange` to every bridge (fan-out). The io pump multiplexes N inbound streams instead of one. (One HAL, one store, one interpreter — unchanged.)

### 3.4 One graph model in `arora-behavior`, + edition on the trait

Put the graph model in **`arora-behavior`** (not a new crate — per decision): generalize the BT node into a shared `Graph`/`Node` where a node binds to a **function that is either a statically-known id or a module call** (exactly the `function: Uuid` + `tick_builtin`-or-`arora_call` split BT already has), with **Value-typed I/Os** and **links** between I/Os (generalizing `Expression::NodeArgument`). Vizij's graph and the BT become two **interpreters** over this one model; each reads links its own way. Add edition to the trait:
```rust
pub trait BehaviorInterpreter {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError>;
    fn apply(&mut self, diff: GraphDiff) -> Result<(), BehaviorError>; // new: add/remove nodes & links, set/override predetermined keys
}
// "load" == apply a diff onto an empty graph.
```

### 3.5 Behavior edition reachable through the engine (no bypass)

`arora-behavior` exposes a **well-known ("golden") module reference** + function id(s) for the edit operation(s) (e.g. `apply_graph_diff`). These are **registered to the engine as statically-known functions** (the same mechanism as the native BT control nodes) whose implementation applies the diff to the device's interpreter. Then: **wire the stubbed `BridgeOp::Call` → `Engine` dispatch** (`runtime.rs:432-435`); a remote `AroraCall` to the golden id flows the *normal* path (bridge → engine dispatch → the registered edit function → `interpreter.apply(diff)`). No special-casing of `handle_command` for the interpreter.

### 3.6 Predetermined I/Os overridden by links

A node I/O may carry an optional predetermined key (its default store binding — the animation track's authored key, a sink node's `TypedPath`). The interpreter binds the I/O to that key **unless a link overrides it**. Setting/overriding is done *with links* in the graph diff — no separate mechanism.

---

## 4. Impact on the Vizij orchestrator migration

Once arora carries this model, the orchestrator migration largely evaporates. The orchestrator "really resembles arora"; after the switch, **all that remains is the tick sequence between animation (predetermined keys) and the node graph** — which is exactly *an animation node (a `ModuleCall` to the animation module) producing values at their predetermined keys, with links wiring those keys into the rest of the graph.* The Step-C `ModuleCall` node + the animation module's `default_key` outputs are already the two halves of that; Step D becomes "represent the orchestrator as one graph in the shared model, with the animation node's predetermined outputs linkable," not a bespoke interpreter.

## 5. Sequencing (PRs, unmerged — for review)

Do the arora cleanup **before** finishing the orchestrator. Suggested reviewable PRs, stacked:
1. **Builder + fold `Runtime` into `Arora`**; collapse the `run*`/`with_io*` variants; delete the dead `Arora::run_*`; `with_data_store(dyn)`. (Behavior-preserving where possible; `arora-web`/`run.rs`/`main.rs`/examples updated to the builder.)
2. **`step`/`run` legible spine** + one `dt`/`StepOutcome` convention across native/web.
3. **Multiple bridges** (fan-in/fan-out).
4. **Graph model into `arora-behavior`** + `BehaviorInterpreter::apply(GraphDiff)`; the BT becomes an interpreter over it.
5. **Behavior edition through the engine**: golden edit-function registration + wire `BridgeOp::Call` → engine dispatch.
6. **Predetermined I/Os + link override** (small, on top of the graph model).

Each is a breaking change on a young API; per Semio policy, real MAJOR bumps with dependents re-pinned in lockstep. **Do not merge without Victor's review.**

## 6. Open questions

- **Q-A — behavior-queue semantics.** Today STEP 2 round-robins a `VecDeque` of behaviors (`runtime.rs:352-373`). The target is *one* injected interpreter. Confirm we drop multi-behavior round-robin entirely (a "run several behaviors" need would then be expressed *inside* one graph, not by the runtime).
- **Q-B — how the golden edit-function reaches the interpreter.** The engine-registered edit function needs a handle to the device's interpreter. Cleanest is for the builder to wire that registration against the injected interpreter at `build()`. Confirm that's acceptable (vs. the runtime intercepting the golden module id post-dispatch).
- **Q-C — vizij `Value` vs arora `Value`.** The shared graph model types I/Os as arora `Value`. Vizij keeps its own `Value`+`Shape` and converts at the interop boundary (as today). Confirm we are *not* unifying the two value enums in this pass (only the graph structure).
- **Q-D — expressions in links.** You noted "expressions may sneak into links eventually." BT already has `Expression`. Confirm expressions-on-links are out of scope for this pass (structure + predetermined keys only), to be added later.
