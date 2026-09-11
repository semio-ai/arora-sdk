# Inventory: every module occurrence

Where modules are declared, generated, registered or transcribed, across
arora-sdk and vizij-rs. "Declared" is where the interface is written; "ids"
is where they are (re)typed by hand.

## arora-sdk

| Where | Kind | Declared | Generated | Ids by hand |
|---|---|---|---|---|
| `modules/polly` | native cdylib, Rust | `module.yaml` | `build.rs` → `src/arora_generated/` (shims, codec, `*_RAW_ID`) | — |
| `modules/test-rust-wasm` | wasm guest, Rust | `module.yaml` | same | — |
| `modules/test-behavior-tree-nodes` | wasm guest, Rust; mutable params, `children` | `module.yaml` | same | — |
| `modules/test-rust-component` | wasm component (wasip2), Rust | WIT world, no `module.yaml` | `wit_bindgen::generate!` | function ids as `Id { hi, lo }` consts |
| `modules/nao`, `modules/test-cpp`, `modules/test-cpp-2`, `examples/modules/cpp` | C++ | `module.yaml` | `arora-module-cpp` | — |
| `crates/arora-behavior/src/interpreter_module.rs` | host module, no artifact | Rust consts | — | `ID`, `EDIT`, `LOAD`, `SPAWN`, `HALT` + arg ids; encode/decode by hand |
| `crates/arora/src/lib.rs` (builder) | registers the interpreter as a `HostModule` | — | — | uses the consts above |
| `crates/arora-behavior-tree/src/schema.rs` | `_RET_PARAM_ID` | Rust const | — | yes |
| `crates/arora-behavior-tree` | generated **types** only (`Status`, `TickId`) from `declare_*` factories | Rust factories | `build.rs` → `src/arora_generated/` | — |
| `crates/arora/src/module_discovery.rs` | freezes a guest header's primitive-only exports into `frozen::Function` for `DescribeMethods` | — | — | — |
| `crates/arora-bridge-ros2/src/services.rs`, `actions.rs` | consumes `frozen::Function` (parameters, ordering, return) to synthesize ROS services/actions | — | — | — |
| `crates/arora-module-authoring/{core,rust,cpp,cli}` | the generator: YAML → registry → assets → Rust/C++ sources + stripped header | — | — | — |
| `tests/` (integration) | loads built artifacts through `arora-cli` with their headers | — | — | — |

## vizij-rs

| Where | Kind | Declared | Generated | Ids by hand |
|---|---|---|---|---|
| `crates/interop/vizij-animation-module` | wasm guest **and** host-registered, Rust | `module.yaml` + `types/*.yaml` records | `build.rs` → `src/arora_generated/` + `records/` | `pub mod ids` (module + 13 fn + 2 type ids), `tests/host_ramp.rs` (all ids incl. params, as strings) |
| `crates/vizij/src/animation.rs` | `HostModule` over the crate above | — | — | consumes `ids`; reads arguments **by position** ([VIZ-129](https://linear.app/semio-ai/issue/VIZ-129/the-animation-host-module-reads-call-arguments-by-position-not-by)) |
| `crates/interop/vizij-arora-tts` | host module (`say`) | Rust consts + `say_signature()` built by hand | — | `MODULE_ID`, `SAY_ID`, param ids |
| `crates/vizij/src/tts_piper.rs` | host module (`say`, same signature) | same | — | `MODULE_ID` |
| `crates/interop/vizij-arora-behavior/src/speech.rs` | the shared `say` signature | `say_signature()` by hand (frozen `Function`, `Status@1.0.0`) | — | `SAY_*_PARAM_ID` |
| `crates/vizij/src/viseme.rs`, `vizij-arora-behavior/src/viseme.rs` | host module (`play_viseme`) | consts + signature by hand | — | `MODULE_ID`, `PLAY_VISEME_ID` |
| `crates/vizij/src/gaze.rs`, `vizij-arora-behavior/src/gaze.rs` | host module (`look_at`) | signature by hand; ids **name-hashed** (`gen_uuid_from_str("gaze-module")`) | — | (hashed) |
| `crates/vizij/src/device.rs` | registers the host modules; tests build ad-hoc ones (`GAZE_MODULE`, `PROVIDER`) | — | — | test consts |
| `crates/vizij/src/ros2_tests.rs` | ad-hoc host module (`speak`), name-hashed ids | — | — | (hashed) |
| `crates/node-graph/vizij-graph-core` (`ExternalFunction`) | **calls** modules: `function` + `param_ids` authored in the graph, zipped with variadic inputs | — | — | graph-authored |
| `crates/interop/vizij-arora-host/src/lib.rs` | authors graph nodes with `FN_STEP` / `PARAM_DT_NS` strings | — | — | yes |
| `crates/interop/vizij-arora-host/src/skills.rs` | `SAY_ID`, `SAY_*_PARAM_ID`, skill function names | Rust consts | — | yes |

## Outside Rust

| Where | What |
|---|---|
| vizij-web `packages/@vizij/runtime-react/src/engine/animationModule.ts` | `ANIMATION_MODULE_FN`, `ANIMATION_MODULE_PARAM`, `ANIMATION_MODULE_TYPE` — the animation module's ids, transcribed |
| semio-db `src/gql/v0/{types,query,mutation}/module*.rs` | the store's module record: `record::module::{unfrozen,frozen}::Module` served over GraphQL |

## What the count says

One module — the animation module — has its ids written in **five** places
(`module.yaml`, `ids`, `host_ramp.rs`, `vizij-arora-host`, vizij-web). Every
host module in vizij builds its frozen `Function` by hand, three of them for the
same `say`. Two use name-hashed ids. None of the host modules has a header, so
none can be exported to a `module.yaml` or a store record.

## Findings beside the study

Seen while reading, not part of the mechanism:

- **`NativeExecutor` reads a result's size big-endian** (`native.rs`:
  `u32::from_be_bytes`) where `BufferWriter::finalize` writes it little-endian
  and the wasm executor reads it little-endian. Any native module returning a
  buffer under 16 MiB would be misread; nothing in the SDK's tests drives the
  native executor, which is why it has not shown.
