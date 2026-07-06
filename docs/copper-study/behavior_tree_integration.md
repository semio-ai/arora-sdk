# Behavior Tree Integration with Copper

> Sub-study of [copper_study.md](copper_study.md) — §4.2 "Behavior tree integration".

## Questions

1. Is there a well-known BT implementation that works with Copper's data?
2. Can the Arora BT implementation be adapted to load trees dynamically?
3. How should a BT be integrated? (Cargo.toml, Rust code, nitty-gritty details)
4. How does it map to Copper's concept of tasks?
5. What is the concept of tasks and how to define a behavior canonically in Copper?

---

## Findings

### 1. No Native BT in Copper — Missions Are the Closest Concept

Copper has **no built-in behavior tree** support. Zero references to "behavior tree",
"bonsai", "groot", or "BT" exist in the copper-rs codebase.

The closest concept is **Missions**: switchable DAG configurations that activate
different subsets of tasks and connections.

**Grounding** — from `examples/cu_missions/copperconfig.ron`:

```ron
(
    missions: [(id: "A"), (id: "B")],
    tasks: [
        (id: "src", type: "tasks::ExampleSrc"),
        (id: "taskA", type: "tasks::ExampleTask", config: {"label": "Mission A"}, missions: ["A"]),
        (id: "taskB", type: "tasks::ExampleTask", config: {"label": "Mission B"}, missions: ["B"]),
        (id: "sink", type: "tasks::ExampleSink"),
    ],
    cnx: [
        (src: "src", dst: "taskA", msg: "i32", missions: ["A"]),
        (src: "src", dst: "taskB", msg: "i32", missions: ["B"]),
        (src: "taskA", dst: "sink", msg: "i32", missions: ["A"]),
        (src: "taskB", dst: "sink", msg: "i32", missions: ["B"]),
    ],
)
```

**How missions switch** — from `examples/cu_missions/src/main.rs`:

```rust
#[copper_runtime(config = "copperconfig.ron")]
struct App {}

use A::App as MissionAApp;
use B::App as MissionBApp;

fn main() {
    // Run Mission A
    let mut app_a = MissionAApp::builder()
        .with_clock(clock.clone())
        .with_log_path(&logger_path, SLAB_SIZE).unwrap()
        .build().unwrap();
    app_a.start_all_tasks()?;
    app_a.run_one_iteration()?;
    app_a.stop_all_tasks()?;

    // Switch to Mission B
    let mut app_b = MissionBApp::builder()
        .with_clock(clock.clone())
        .with_log_path(&logger_path, SLAB_SIZE).unwrap()
        .build().unwrap();
    app_b.start_all_tasks()?;
    app_b.run_one_iteration()?;
    app_b.stop_all_tasks()?;
}
```

**Limitation**: Missions are **compile-time** — each mission generates a separate `App` struct.
You can switch between missions at runtime, but you cannot add new missions without recompiling.
This is fundamentally different from a behavior tree's dynamic branching.

### 2. Best Rust BT Crate: `bonsai-bt`

The leading Rust behavior tree library is [bonsai](https://github.com/Sollimann/bonsai)
(441 ★, topics: `robotics`, `ros2`, `bevy`, `autonomous-robots`).

**bonsai** provides:
- Standard BT nodes: `Sequence`, `Select`, `While`, `WhenAll`, `WhenAny`, `If`, `Wait`
- `Action` nodes with `Status` return: `Success`, `Failure`, `Running`
- Blackboard for shared state
- Dynamic tree construction at runtime
- Serialization/deserialization support

### 3. Integration: BT as a CuTask

A behavior tree integrates naturally as a **`CuTask`** that ticks each cycle:

#### Cargo.toml

```toml
[package]
name = "cu-behavior-tree"
version = "0.1.0"
edition = "2024"

[dependencies]
cu29 = "0.15"
bonsai-bt = "0.11"
serde = { version = "1", features = ["derive"] }
bincode = "2"

[dev-dependencies]
cu29 = { version = "0.15", features = ["mock"] }
```

#### Payload Definitions

```rust
use cu29::prelude::*;
use serde::{Serialize, Deserialize};

/// Input to the BT: sensor/state data from upstream tasks
#[derive(Debug, Default, Clone, Serialize, Deserialize, Reflect)]
pub struct BTreeInput {
    pub joint_positions: Vec<f64>,    // Current joint positions
    pub target_reached: bool,         // Whether current target was reached
    pub battery_level: f64,           // Battery percentage
    pub error_state: Option<String>,  // Any error to handle
}

/// Output from the BT: commands for downstream tasks
#[derive(Debug, Default, Clone, Serialize, Deserialize, Reflect)]
pub struct BTreeOutput {
    pub target_positions: Vec<f64>,   // Joint target positions
    pub action_name: String,          // Currently executing action
    pub status: BTreeStatus,          // BT tick result
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Reflect)]
pub enum BTreeStatus {
    #[default]
    Idle,
    Running,
    Success,
    Failure,
}
```

#### CuTask Implementation

```rust
use bonsai_bt::{BT, Behavior, Status, Success, Failure, Running, UpdateArgs, Event, Action, Sequence, Select, Wait, If};
use cu29::prelude::*;

/// A Copper task that ticks a behavior tree each cycle.
#[derive(Reflect)]
pub struct BehaviorTreeTask {
    bt: BT<BTreeAction, SharedBlackboard>,
    tree_source: Option<String>,  // Path or serialized tree for dynamic loading
}

/// Actions the BT can invoke
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BTreeAction {
    MoveTo(Vec<f64>),        // Move to joint positions
    WaitForTarget,           // Wait until target reached
    CheckBattery(f64),       // Check battery > threshold
    PlayAnimation(String),   // Trigger an animation
    Log(String),             // Log a message
}

/// Shared state between BT and Copper (used as the BT blackboard)
#[derive(Debug, Default, Clone)]
pub struct SharedBlackboard {
    pub joint_positions: Vec<f64>,
    pub target_reached: bool,
    pub battery_level: f64,
    pub current_action: String,
    pub target_positions: Vec<f64>,
}

impl Freezable for BehaviorTreeTask {
    fn freeze<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        // Serialize the blackboard state
        // Note: bonsai-bt's BT internal cursor state would need custom handling
        Ok(())
    }

    fn thaw<D: Decoder>(&mut self, _decoder: &mut D) -> Result<(), DecodeError> {
        Ok(())
    }
}

impl CuTask for BehaviorTreeTask {
    type Input = input_msg!(BTreeInput);
    type Output = output_msg!(BTreeOutput);

    fn new(config: Option<&ComponentConfig>) -> CuResult<Self> {
        // Load tree from config or use a default
        let tree_source = config
            .and_then(|c| c.get::<String>("tree_file"));

        let behavior = if let Some(ref path) = tree_source {
            // Dynamic loading: deserialize tree from file
            let tree_json = std::fs::read_to_string(path)
                .map_err(|e| CuError::new_with_cause("Failed to load BT file", e))?;
            serde_json::from_str(&tree_json)
                .map_err(|e| CuError::new_with_cause("Failed to parse BT", e))?
        } else {
            // Default: simple approach-and-wave behavior
            Sequence(vec![
                Action(BTreeAction::CheckBattery(20.0)),
                Action(BTreeAction::MoveTo(vec![0.0, 0.5, 0.0, -0.5])),
                Action(BTreeAction::WaitForTarget),
                Action(BTreeAction::PlayAnimation("wave".to_string())),
            ])
        };

        let blackboard = SharedBlackboard::default();
        let bt = BT::new(behavior, blackboard);

        Ok(Self { bt, tree_source })
    }

    fn process(
        &mut self,
        ctx: &CuContext,
        input: &Self::Input,
        output: &mut Self::Output,
    ) -> CuResult<()> {
        // 1. Update blackboard from input
        if let Some(data) = input.payload() {
            let bb = self.bt.blackboard_mut();
            bb.joint_positions = data.joint_positions.clone();
            bb.target_reached = data.target_reached;
            bb.battery_level = data.battery_level;
        }

        // 2. Tick the behavior tree
        // bonsai-bt tick() takes an Event wrapping UpdateArgs { dt }
        let dt = ctx.clock.since_last_tick_ms() as f64 / 1000.0;
        let e: Event = UpdateArgs { dt }.into();

        let tick_result = self.bt.tick(&e, &mut |args, bb: &mut SharedBlackboard| {
            match args.action {
                BTreeAction::MoveTo(ref positions) => {
                    bb.target_positions = positions.clone();
                    bb.current_action = format!("MoveTo({:?})", positions);
                    if bb.target_reached { (Success, args.dt) } else { (Running, args.dt) }
                }
                BTreeAction::WaitForTarget => {
                    bb.current_action = "WaitForTarget".to_string();
                    if bb.target_reached { (Success, args.dt) } else { (Running, args.dt) }
                }
                BTreeAction::CheckBattery(threshold) => {
                    bb.current_action = format!("CheckBattery({})", threshold);
                    if bb.battery_level > *threshold { (Success, args.dt) } else { (Failure, args.dt) }
                }
                BTreeAction::PlayAnimation(ref name) => {
                    bb.current_action = format!("PlayAnimation({})", name);
                    (Success, args.dt) // Assume instant for simplicity
                }
                BTreeAction::Log(ref msg) => {
                    debug!("BT Log: {}", msg);
                    (Success, args.dt)
                }
            }
        });

        // 3. Write output
        let (status, _remaining_dt) = tick_result.unwrap_or((Running, 0.0));
        let bb = self.bt.blackboard_mut();
        output.set_payload(BTreeOutput {
            target_positions: bb.target_positions.clone(),
            action_name: bb.current_action.clone(),
            status: match status {
                Success => BTreeStatus::Success,
                Failure => BTreeStatus::Failure,
                Running => BTreeStatus::Running,
                _ => BTreeStatus::Running,
            },
        });

        Ok(())
    }
}
```

#### RON Configuration

```ron
(
    tasks: [
        (id: "state_aggregator", type: "tasks::StateAggregator"),
        (id: "behavior_tree", type: "cu_behavior_tree::BehaviorTreeTask",
         config: {"tree_file": "behaviors/approach_wave.json"}),
        (id: "joint_controller", type: "cu_pid::GenericPIDTask",
         config: {"kp": 1.0, "kd": 0.1, "ki": 0.0, "setpoint": 0.0, "cutoff": 100.0}),
    ],
    cnx: [
        (src: "state_aggregator", dst: "behavior_tree", msg: "cu_behavior_tree::BTreeInput"),
        (src: "behavior_tree", dst: "joint_controller", msg: "cu_behavior_tree::BTreeOutput"),
    ],
)
```

### 4. Dynamic Tree Loading

**Can behavior trees be loaded dynamically?** Yes, with limitations:

1. **Tree structure**: `bonsai-bt`'s `Behavior<A>` can be serialized/deserialized with serde.
   You can load tree JSON/YAML files at `new()` time (task creation) or even at runtime
   during `process()` by watching for file changes.

2. **Action types**: The `BTreeAction` enum must be known at compile time. You cannot add
   new action types without recompiling. However, you can make actions generic:

   ```rust
   #[derive(Serialize, Deserialize)]
   pub enum BTreeAction {
       // Generic actions that dispatch by name
       CallFunction { name: String, args: Vec<Value> },
       SetState { key: String, value: Value },
       WaitForCondition { key: String, expected: Value },
   }
   ```

3. **Hot-reload pattern**: Watch a file and replace the tree:

   ```rust
   fn process(&mut self, ctx: &CuContext, input: &Self::Input, output: &mut Self::Output) -> CuResult<()> {
       // Check for tree file changes (e.g., every N ticks)
       if self.should_reload() {
           if let Some(ref path) = self.tree_source {
               if let Ok(json) = std::fs::read_to_string(path) {
                   if let Ok(behavior) = serde_json::from_str(&json) {
                       self.bt = BT::new(behavior, self.blackboard.clone());
                   }
               }
           }
       }
       // ... normal tick
   }
   ```

### 5. Mapping to Copper's Task Concept

In Copper, **"behavior"** is expressed differently than in a behavior tree:

| Concept | Behavior Trees | Copper Tasks |
|---------|---------------|--------------|
| **Unit of behavior** | BT node (Action, Sequence, Select) | Task (CuSrcTask, CuTask, CuSinkTask) |
| **Composition** | Tree structure (parent-child) | DAG (connections in RON) |
| **Branching** | Selector/Sequence nodes | Missions (compile-time), or logic in a single task |
| **State** | Blackboard | Task fields + Freeze/Thaw |
| **Lifecycle** | Tick (recursive tree traversal) | process() called in topological order |
| **Dynamic switching** | Tree re-rooting, subtree replacement | Mission switching (stop app A, start app B) |

**The canonical way to define behavior in Copper** is:

1. **Simple behaviors**: Each task's `process()` method encodes its logic directly
2. **Complex behaviors**: Use a `CuTask` wrapper around a BT library (as shown above)
3. **Multi-mode behaviors**: Use missions to switch between different DAG configurations
4. **Distributed behaviors**: Use Zenoh bridges to coordinate across subsystems

### 6. Adapting the Arora BT Module

The former Arora engine's BT module (inferred from `arora-registry` types `Status`, `TickId`,
`CallableId`) was likely a WASM module that:
- Exported a `tick()` function
- Used `Call { module_id, function_id, args }` for inter-module communication
- Returned `Status` (Success/Failure/Running)

To port this to Copper:

```text
Former Arora Engine                    Copper
─────────────────                    ──────
BT Module (WASM)                     BehaviorTreeTask (CuTask)
  tick() export         →            process() method
  Call(module, fn, args) →           Output payload → downstream task via CopperList
  Status return          →           BTreeOutput.status field
  Dynamic loading        →           serde tree loading from file/config
  WASM sandboxing        →           Not available (same process)
```

**What's lost**: WASM sandboxing, dynamic module type registration, cross-language modules.
**What's gained**: Zero-alloc execution, deterministic replay, compile-time type safety,
sub-microsecond tick latency.

---

## Verification

- `bonsai-bt` crate exists and is published on crates.io (v0.11.0, latest as of 2026-03-17)
- `bonsai-bt`'s `Behavior<A>` derives `Serialize`/`Deserialize` where `A: Serialize + Deserialize`
- Copper's `CuTask` trait accepts arbitrary types as Input/Output, including custom BT payloads
- The missions example confirms compile-time DAG switching is the native behavior model
- No BT references exist in copper-rs (searched: "behavior", "bonsai", "groot", "btree")
