# Async functions in the module system: where we stand

A study of what it would take to support genuinely asynchronous module
functions, anchored on the one real long-running function we ship today:
`polly::say`.

## How `say` works today

`modules/polly` is a **host cdylib** (native executor). Its `say` function
(`modules/polly/src/lib.rs`) cannot block the engine tick, so it hand-rolls
a poll-on-tick state machine:

1. First call: spawns the AWS Polly synthesis + playback on a lazily
   created tokio runtime, stores the `JoinHandle` in a global, returns
   `Status::Running`.
2. Subsequent ticks: return the cached status; the spawned task flips it to
   `Success`/`Failure` when done.
3. The behavior tree's `Running` semantics act as the scheduler: the tree
   re-ticks the node until it resolves.

This works, but the workaround carries real defects:

- **Global singleton state.** `TTS_TASK`/`TTS_STATUS` are process-wide:
  two `say` nodes (or two trees) interleave their statuses and one
  utterance wins. The state should be keyed per node/callable.
- **Status hand-off is racy by design.** "The task finished but the status
  still needs to be reported next call" (lib.rs:69) means a `Success` can
  be lost if a different node ticks first, and a re-trigger in the same
  tick window silently coalesces.
- **A blocking sleep inside the async task.** Playback waits with
  `std::thread::sleep` in a loop (lib.rs:103), pinning a tokio worker
  thread for the duration of the audio. `tokio::time::sleep` (or
  `spawn_blocking` for the whole playback) is the correct form.
- **Only native modules can do this at all.** A wasm32-wasip1 guest has no
  threads and no runtime: a wasm `say` would block the entire engine
  inside `dispatch` until synthesis finished. This is why polly is
  native-only today — the capability gap is the module ABI, not polly.

## The layers between us and async

From the bottom up:

| Layer | Today | For async |
| ----- | ----- | --------- |
| Guest ABI (wasip1 core) | sync `arora_function_<uuid>(ptr) -> ptr` | dead end — no threads, no futures; polling is the ceiling |
| Guest ABI (component, wasip2) | sync `dispatch(method, arg) -> result` (`wit/arora-module.wit`) | same shape, but the host side can already drive stores with wasmtime's async support |
| Guest ABI (component, wasip3) | — | functions become `async func`; `future<T>`/`stream<T>` exist in WIT; wasmtime surfaces them as Rust futures |
| Host imports (`host.dispatch`) | sync, re-entrant via raw `EngineRef` | async imports remove the re-entrancy problem: a guest awaiting `host.dispatch` suspends instead of re-entering the borrowed engine |
| `Engine::dispatch` | sync `&mut self`, re-entrant | becomes `async`; module calls return futures; the raw-pointer aliasing documented in design_decisions.md dissolves because re-entry becomes suspension |
| Behavior tree | `Running` + re-tick = cooperative scheduling | unchanged conceptually — `Running` maps onto "future not ready"; the tick loop polls an invocation table instead of re-calling guest code |
| Browser host | sync JS calls into `arora-web` | needs either jco-style async transpilation (JSPI — Chrome yes, Firefox not yet) or callback/promise plumbing in the browser executor |

The striking alignment: **the behavior tree already has async semantics**
(`Running`), it just lacks a substrate. Every piece of polly's workaround
is a manual implementation of what WASIp3 provides natively.

## Distance assessment

- **Stage 0 — now, small.** Fix polly's known defects within the polling
  contract: key task/status by callable id, replace the blocking sleep,
  document "long-running functions return `Running` and are re-ticked" as
  the official module contract. No engine changes. (~1 day)
- **Stage 1 — components on wasip2, medium.** Move long-running modules to
  the component executor and turn on wasmtime's async config
  (`Config::async_support`, `call_async`, `add_to_linker_async`). Host
  imports become async (the engine can await I/O inside `host.dispatch`),
  guests remain sync. `Engine::dispatch` grows an async variant; the CLI
  already runs under tokio. This is the structural rehearsal for p3 and
  removes the engine-blocking risk for host-side work. (~1–2 weeks,
  mostly Engine/CallBridge API surface)
- **Stage 2 — wasip3, the real thing.** `arora:module` functions become
  `async func`; `say(text) -> status` suspends in the guest across the
  synthesis await. Engine keeps an invocation table (callable id →
  future); the BT tick polls it. Blocked on externals: wasmtime's `p3`
  module is experimental (no semver guarantees), `wasm32-wasip3` is Tier 3
  in rustc, and Firefox lacks JSPI for the browser path. Re-evaluate when
  WASI 1.0 lands (roadmap: late 2026 / early 2027). The WIT world was
  shaped so this stage changes signatures, not architecture.

## Recommendation

Do Stage 0 opportunistically (it fixes user-visible polly bugs). Schedule
Stage 1 as the follow-up to the component migration — it delivers most of
the practical value (engine never blocks on module I/O) while staying on
stable toolchains. Treat Stage 2 as a target to track, not to build on
yet.
