# WASM Support and Browser Simulation

> Sub-study of [copper_study.md](copper_study.md) — §4.1 "WASM and browser simulation".

## Question

Can Copper run in the browser via WASM? What does it mean to write a controller
in that case? Do we have any supported simulation I can run in a browser?
Bevy? ThreeJS? Anything? Could you point at actual recipes to put that together?

---

## Findings

### 1. Copper Runs in the Browser — Confirmed

Since v0.14 (changelog: *"Webassembly as a new target! Copper can run directly in browsers!"*),
Copper compiles to `wasm32-unknown-unknown` and runs in the browser. Live demos are hosted at
`cdn.copper-robotics.com`.

**Grounding**: The `cu_rp_balancebot` and `cu_flight_controller` examples both have browser
build targets. From the balancebot `justfile`:

```makefile
web:
    cd {{justfile_directory()}} && trunk serve --open

web-dist:
    cd {{justfile_directory()}} && trunk build --release
```

And from `cu_bevymon_demo/src/main.rs`, WASM-conditional compilation:

```rust
#[cfg(target_arch = "wasm32")]
type LogSectionStorage = NoopSectionStorage;
#[cfg(not(target_arch = "wasm32"))]
type LogSectionStorage = MmapSectionStorage;

#[cfg(target_arch = "wasm32")]
fn build_logger(_: &PathBuf) -> CuResult<UnifiedLoggerWrite> {
    Ok(NoopLogger::new())
}
```

Source: [cu_bevymon_demo/src/main.rs](https://github.com/copper-project/copper-rs/blob/master/examples/cu_bevymon_demo/src/main.rs)

### 2. What Writing a Controller for WASM Means

In Copper, there is no special "WASM controller" concept. You write a standard `CuTask` (or
`CuSrcTask` / `CuSinkTask`) and the same code compiles for native and WASM:

```rust
#[derive(Reflect)]
pub struct MyPIDController {
    kp: f64,
    kd: f64,
    ki: f64,
    // ...
}

impl Freezable for MyPIDController {}

impl CuTask for MyPIDController {
    type Input = input_msg!(SensorReading);
    type Output = output_msg!(MotorCommand);

    fn new(config: Option<&ComponentConfig>) -> CuResult<Self> {
        let kp = config.and_then(|c| c.get::<f64>("kp")).unwrap_or(1.0);
        // ...
        Ok(Self { kp, kd, ki, .. })
    }

    fn process(&mut self, _ctx: &CuContext, input: &Self::Input, output: &mut Self::Output) -> CuResult<()> {
        if let Some(reading) = input.payload() {
            let error = self.setpoint - reading.value;
            let command = self.kp * error + self.kd * self.d_error + self.ki * self.i_error;
            output.set_payload(MotorCommand { value: command });
        }
        Ok(())
    }
}
```

The **differences for WASM** are only in the infrastructure:

| Aspect | Native | WASM |
|--------|--------|------|
| Logger | `MmapSectionStorage` (mmap file) | `NoopSectionStorage` / `NoopLogger` |
| Clock | `RobotClock::default()` | `RobotClock::default()` (uses `web-time`) |
| Build tool | `cargo build` | `trunk serve` / `trunk build --release` |
| Hardware I/O | Real GPIO, SPI, etc. | Simulated in Bevy (physics engine) |

**Key insight**: In the browser, hardware sources and sinks become Bevy simulation components.
The processing tasks (PID, AHRS, etc.) remain identical. This is achieved through the
**simulation callback** (`cu_run_in_sim` pattern):

```rust
// Simulation callback: replace hardware with Bevy physics
fn sim_callback(step: &SimStep, sim_state: &mut SimState) -> SimOverride {
    match step {
        SimStep::Process("sensor_task") => {
            // Read from Bevy physics engine instead of real hardware
            let reading = sim_state.bevy_world.get_sensor();
            step.set_output(SensorReading { value: reading });
            SimOverride::ExecutedBySim
        }
        SimStep::Process("motor_task") => {
            // Write to Bevy physics engine instead of real motor
            let command = step.get_input::<MotorCommand>();
            sim_state.bevy_world.apply_motor(command);
            SimOverride::ExecutedBySim
        }
        _ => SimOverride::ExecuteByRuntime // Real code for processing tasks
    }
}
```

### 3. Supported Browser Simulation: Bevy + Avian3D

The browser simulation stack is:

```text
Copper Runtime (WASM)
  └── Bevy 0.18 (game engine, compiles to WASM via wasm-bindgen)
       └── Avian3D 0.6.1 (physics engine)
            └── WebGL2 / WebGPU rendering
```

**Concrete examples with browser simulation:**

1. **BalanceBot** (`cu_rp_balancebot`):
   - Balance PID + rail position PID → motor control
   - Bevy scene with rigid-body cart on rails
   - Physics simulated by Avian3D
   - Runs in browser: `trunk serve --open`
   - Source: [cu_rp_balancebot](https://github.com/copper-project/copper-rs/tree/master/examples/cu_rp_balancebot)

2. **Flight Controller** (`cu_flight_controller`):
   - Full quadcopter: IMU → AHRS → attitude PID → rate PID → quad mixer → ESCs
   - Bevy 3D scene with quadcopter model
   - OSD overlay, split-view with `cu_bevymon`
   - Runs in browser: `trunk serve --open`
   - Source: [cu_flight_controller](https://github.com/copper-project/copper-rs/tree/master/examples/cu_flight_controller)

3. **BevyMon Demo** (`cu_bevymon_demo`):
   - Minimal 3D scene + Copper monitor side-by-side
   - Shows the split-layout pattern for combining sim + monitoring
   - Source: [cu_bevymon_demo](https://github.com/copper-project/copper-rs/tree/master/examples/cu_bevymon_demo)

### 4. ThreeJS — Not Supported

There is no ThreeJS integration in the Copper ecosystem. Copper's browser rendering
is entirely through **Bevy** (which uses wgpu → WebGL2/WebGPU). ThreeJS is a JavaScript
3D library and would require a separate rendering bridge, which Copper does not provide.

**If you wanted ThreeJS**: You would need to extract the Copper runtime state from WASM
and bridge it to a ThreeJS scene via JavaScript interop (`wasm-bindgen` + JS glue code).
This is doable but not supported out of the box. Bevy is the recommended path.

### 5. Recipe: Adding a Copper WASM Simulation to This Project

To build a browser-runnable Copper simulation from the current studio-bridge codebase:

**Step 1: Cargo.toml**

```toml
[package]
name = "arora-copper-sim"
version = "0.1.0"
edition = "2024"

[dependencies]
cu29 = "0.15"
cu_pid = "0.15"
bevy = "0.18"
cu_bevymon = "0.15"
avian3d = "0.6"

[target.'cfg(target_arch = "wasm32")'.dependencies]
cu29 = { version = "0.15", features = ["wasm"] }
```

**Step 2: copperconfig.ron**

```ron
(
    tasks: [
        (id: "joint_sensor", type: "tasks::JointSensor"),
        (id: "joint_pid",    type: "cu_pid::GenericPIDTask",
         config: {"kp": 1.0, "kd": 0.1, "ki": 0.0, "setpoint": 0.0, "cutoff": 100.0}),
        (id: "joint_motor",  type: "tasks::JointMotor"),
    ],
    cnx: [
        (src: "joint_sensor", dst: "joint_pid", msg: "payloads::JointReading"),
        (src: "joint_pid", dst: "joint_motor", msg: "cu_pid::PIDControlOutputPayload"),
    ],
    monitor: (type: "cu_bevymon::CuBevyMon"),
)
```

**Step 3: Build for browser**

```bash
# Install trunk
cargo install trunk

# Build and serve
trunk serve --open
```

**Step 4: For distribution**

```bash
trunk build --release
# Output in dist/ — deploy to any static hosting
```

### 6. Implications for Arora

The current `headless` binary cannot run in the browser — it depends on `tokio`,
`rustls`, Firebase authentication, and WebSocket networking. With Copper:

- **Processing tasks** (PID controllers, animation engines) compile to WASM unchanged
- **Hardware I/O** is replaced by Bevy physics simulation in the browser
- **Monitoring** uses `cu_bevymon` for in-browser TUI display
- **Networking** would need a separate Zenoh bridge (Copper's Zenoh bridge supports WASM)

This would allow a **"digital twin"** where the same control code runs on the real robot
and in the browser, with Bevy providing the 3D visualization.

---

## Verification

The WASM support claim is verified by:

1. **Source code**: `#[cfg(target_arch = "wasm32")]` conditionals in `cu_bevymon_demo/src/main.rs`
2. **Build targets**: `trunk serve --open` in multiple example `justfile`s
3. **Live demos**: Hosted at `cdn.copper-robotics.com` (referenced in wiki changelog)
4. **Bevy 0.18**: Known to compile to WASM with wgpu WebGL2/WebGPU backend
5. **`NoopLogger`**: Explicit WASM-compatible logger replacement, confirming no filesystem dependency
