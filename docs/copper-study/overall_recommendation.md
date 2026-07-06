# Overall Recommendation: Should We Switch to Copper?

> Sub-study of [copper_study.md](copper_study.md) — §4.7 "Overall recommendation".

## Question

All in all, would it be worth it switching to Copper for this use?

---

## Analysis

### What We Have Today

The current `studio-bridge` architecture is a **simple, async, key-value messaging system**:

```text
Studio (browser) ←WSS→ Bridge Server ←WSS→ Device Client ←mpsc→ Controller
                                                              ↕
                                                         AnimationEngine
```

**Strengths**:
- Simple to understand (~500 LoC engine, ~700 LoC controller)
- Single async loop, easy to debug
- Works reliably for remote device control from Studio
- Generic key-value data model (any sensor/actuator via `StateChange`)
- Cross-platform (compiles on ARM, x86, WASM for Studio client)
- Already deployed on real robots (NAO, Quori, UR3, Pepper)

**Weaknesses**:
- No deterministic replay
- No structured processing pipeline (one monolithic controller)
- No simulation mode (fake controller is basic)
- No visualization framework
- No standardized task lifecycle
- Feature-flag composition is inflexible
- No standard way to add processing stages (e.g., filters, behavior trees)

### What Copper Would Bring

| Capability | Benefit for Arora | Cost |
|-----------|------------------|------|
| **DAG task graph** | Structured pipeline: sensor → filter → BT → PID → actuator | Rewrite engine and controller as tasks |
| **Zero-alloc CopperList** | Sub-microsecond internal latency | Strict type constraints, no dynamic keys |
| **Deterministic replay** | Record & replay exact robot behavior | All state must be serializable (Freezable) |
| **Bevy simulation** | In-browser digital twin | New dependency, significant integration work |
| **cu_bevymon** | Runtime monitoring | Bevy dependency |
| **ROS 2 via Zenoh** | Standard robot middleware support | Already have ros2_client, would switch to Zenoh-based bridge |
| **Missions** | Multi-mode behavior switching | Compile-time, less flexible than dynamic modules |
| **WASM** | Same control code in browser | Already have Studio WASM client (different purpose) |
| **Zenoh integration** | First-class distributed communication | Natural fit with proposed Zenoh migration |

### What Copper Would Cost

| Cost | Impact |
|------|--------|
| **No dynamic loading** | Cannot replicate former engine's module system |
| **Compile-time DAG** | Cannot add/remove sensors or processing stages at runtime |
| **Strong typing** | Cannot use `HashMap<Key, Option<Value>>` for flexible state |
| **Determinism constraints** | All tasks must be `Freezable`, no unbounded allocations |
| **Learning curve** | New framework, proc macros, RON config, Bevy |
| **Dependency weight** | `cu29` + `bevy` + `zenoh` + `avian3d` = significant compile time |
| **Migration effort** | Rewrite controller trait, engine loop, message types |
| **Maturity** | v0.15.0, active but early-stage (bugs, breaking changes likely) |

### Decision Framework

#### If the primary use case is **Studio ↔ Device remote control**:

**Don't switch.** The current architecture is simpler, more flexible, and sufficient.
The `StateChange` key-value model maps naturally to Studio's needs. Copper's strengths
(deterministic replay, sub-µs latency) aren't valuable for a cloud-connected device
management system.

#### If the primary use case is **on-robot control pipeline**:

**Consider switching.** Copper's DAG model, deterministic replay, and simulation support
would significantly improve the robot control stack. The migration path:

1. **Phase 1**: Keep Studio bridge as-is. Add a Copper pipeline for the on-robot processing.
   Connect them via Zenoh.
2. **Phase 2**: Replace the `StudioBridgeController` trait with a Copper `cu_zenoh_bridge`
   that publishes state to Zenoh, where the existing Studio bridge can consume it.
3. **Phase 3**: Add Bevy simulation for in-browser digital twin.

#### If the primary use case is **dynamic module composition** (like the former engine):

**Don't switch.** Copper fundamentally cannot support dynamic module loading.
If this is a hard requirement, either:
- Keep the current architecture and build the module system on top of it
- Use Copper for the low-level pipeline and a separate module system for orchestration
- Wait for Copper to potentially add plugin support (no roadmap evidence for this)

### Migration Complexity Estimate

| Component | Current | Copper Equivalent | Effort |
|-----------|---------|-------------------|--------|
| `StudioBridgeController` trait | `update()`, `get()`, `subscribe()` | `CuSrcTask` (sensor) + `CuSinkTask` (actuator) | Medium |
| Engine loop (`engine::run()`) | `tokio::select!` with 5 branches | `#[copper_runtime]` + per-task `process()` | High |
| `StateChange` / `Key` / `Value` | Generic key-value | Per-connection typed messages | High |
| `AnimationEngine` | Standalone, ticked at 10ms | `CuTask` wrapping animation-player | Medium |
| Firebase + WSS client | `DeviceClient` trait | `cu_zenoh_bridge` + Zenoh → Firebase adapter | High |
| Feature flags for robots | `#[cfg(feature = "quori")]` | RON config + modular includes | Medium |
| ROS 2 integration | `ros2_client` crate | `cu_ros2_bridge` over Zenoh | Medium |
| Tests | Integration tests with test server | Copper mock + simulation tests | High |

**Total estimated effort**: 2-4 weeks for a basic port, 2-3 months for full feature parity.

### Specific Recommendations

1. **Zenoh first, Copper later**: The zenoh-study (PR #28) is a more natural next step.
   Zenoh alone solves the distributed communication problem without requiring a framework change.
   Copper can be layered on top of Zenoh later.

2. **Copper for new components**: If adding new processing stages (BT, AHRS, IK), write them
   as Copper tasks from the start. Keep the existing bridge as-is and connect via Zenoh.

3. **Bevy for simulation**: If you want in-browser simulation, Copper + Bevy is the path.
   But you could also use Bevy independently without Copper.

4. **Don't rewrite the bridge**: The Firebase + WSS + Studio bridge is complex, tested,
   and works. Replacing it with Copper would be high-risk, low-reward.

---

## Final Verdict

### Is it worth switching to Copper?

**Not as a wholesale replacement.** The studio-bridge architecture serves a different
purpose (device management and remote control) than what Copper excels at (real-time
control pipelines). Copper's strengths don't align with the current system's primary value.

**Yes, as a complementary tool.** For the **on-robot control pipeline**, Copper is
compelling. The ideal architecture is:

```text
┌─────────────────────────────────────────┐
│ Robot                                    │
│                                          │
│  ┌──────────────────────────────────┐   │
│  │ Copper Pipeline                   │   │
│  │ Sensor → Filter → BT → PID →     │   │
│  │  ... → Actuator                   │   │
│  │  ↕ cu_zenoh_bridge               │   │
│  └──────────────────────────────────┘   │
│         ↕ Zenoh pub/sub                  │
│  ┌──────────────────────────────────┐   │
│  │ Studio Bridge (existing)          │   │
│  │ DeviceClient ← engine → Firebase │   │
│  │ (subscribes to Zenoh state)       │   │
│  └──────────────────────────────────┘   │
└─────────────────────────────────────────┘
         ↕ WSS
┌─────────────────────────────────────────┐
│ Cloud                                    │
│ Bridge Server → Studio (browser)         │
└─────────────────────────────────────────┘
```

This gives you:
- ✅ Copper's DAG execution, replay, and simulation for the control pipeline
- ✅ Existing Studio bridge for device management
- ✅ Zenoh as the common data layer
- ✅ Incremental migration (no big-bang rewrite)
- ✅ Bevy simulation available in browser alongside Studio

---

## Summary Table

| Question | Answer |
|----------|--------|
| Worth switching entirely? | **No** — too costly, loses dynamic flexibility |
| Worth using alongside? | **Yes** — excellent for on-robot control pipeline |
| What to do next? | Zenoh migration first (PR #28), then Copper for new pipeline components |
| Can it replace the former engine? | **Partially** — better for real-time control, worse for dynamic modules |
| Risk level | Low if adopted incrementally, high if wholesale replacement |
