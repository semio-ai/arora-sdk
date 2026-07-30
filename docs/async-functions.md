# Long-running and async module functions

Most module functions return in one tick. Some cannot: a text-to-speech
synthesis, a model forward pass, a network request, a behavior that plays out
over seconds. Arora runs a **synchronous, single-owner step loop**, so such a
function may never block it. This page is the contract for writing one.

For the assessment of how far the module *ABI* is from native async — the
substrate roadmap (wasip1 → wasip2 → wasip3) and the concrete defects of the
first real case — see the companion [`async-modules-study.md`](async-modules-study.md).
This page is the normative "how it works today and stays working."

## The contract: return `Running`, be re-ticked

A long-running function returns `behavior_tree.Status::Running` and is
**invoked again next step**. It keeps returning `Running` until it reaches a
terminal `Status` (`Success`/`Failure`). The interpreter that hosts it — a
behavior tree, a node graph — re-ticks it every step for as long as it is
`Running`, and stops when it goes terminal.

That is the whole mechanism, and it is worth seeing for what it is: **`Running`
is `Poll::Pending`.** The function is a hand-written `Future`, the tick loop is
its executor, and one invocation is one `poll`. `Success`/`Failure` is
`Poll::Ready`. Nothing about the step loop changes; a slow computation is
expressed as *many cheap polls*, not one long call.

```
tick 0:  say("hello") -> Running     // kick off the work; nothing to report yet
tick 1:  say("hello") -> Running     // still going
…
tick N:  say("hello") -> Success     // done; the run is dropped
```

## Why polling, and not `await`

The step loop is synchronous, single-owner (one writer of the store, no locks in
the control path), and deterministic. Two things you might reach for both break
it, and polling avoids both:

- **`await` inside the call.** Awaiting would either block the single owner
  (stalling every other run under the same `tick()`) or make the whole tick
  async (losing the deterministic, in-place evaluation). And core wasm has no
  stack switching — a guest function *cannot* suspend mid-call regardless. A
  synchronous `poll` driven from the loop is precisely how you advance async
  work *from* a synchronous, non-suspending substrate.
- **Slicing the computation into per-tick chunks.** A model forward pass has no
  natural yield points; cutting it into tick-sized pieces is fiction. Don't. The
  work runs elsewhere (below); the poll only *observes* it.

Polling a readiness signal each tick is not false decomposition — the tick is
the caller's clock, and "is it ready yet?" is a legitimate per-tick query. What
would be false is doing the *work* in slices. The distinction is **where the
work runs**, not whether you poll.

## Per-invocation state, behind a handle

Because the function is re-entered every tick, its progress state must persist
*between* ticks and must be **keyed per invocation** — never process-global. On
the first tick it starts the work and records a handle; on later ticks it looks
its state up by that handle and advances it; on the terminal tick it reports and
clears it.

Process-global state is the first thing to get wrong: two live invocations of
the same function (two nodes, two trees) would then interleave through one
slot and one would win. Key the state by the callable/run id. A task run's
namespaced keys (see the task-runs design) give exactly this identity.

## Spawn is a host power

The poll must not block — so the genuinely slow work (inference, I/O, playback)
does **not** run inside the function's tick. It runs **off the step thread**,
and the function only polls a channel to it:

- **Native:** a thread or a blocking pool started on the io side; the function
  polls an `mpsc`/`oneshot` (a non-blocking `try_recv`).
- **Browser (wasm):** a **Web Worker** running the work; messages posted back
  land on a channel the function polls.

This is a *parallelism* need, not an async one: even with perfect async support,
`await` lets you wait without blocking — it does not give you a second core to
run a forward pass on. So the off-thread producer exists on every target; the
tick just samples its channel.

The consequence for modules: a **wasip1 guest has no substrate for background
work at all** — no threads, no runtime — so a wasm guest cannot start the
producer itself. Today only **native modules** (host cdylibs) can spawn; a
portable module reaches a **host capability** that owns the spawn and hands back
a channel. Either way the rule holds: *the module stays synchronous; spawning is
the host's job.* The path to letting the guest itself do this (async host
imports on wasip2, async guests on wasip3) is the study's subject and does not
change this contract — `Running` still means pending.

## Building for wasm

The poll *mechanism* is wasm-safe: a hand-written `Future` or a `step` that
returns `Status` is synchronous Rust and compiles to `wasm32` unchanged. What
does *not* survive the port is the **body**.

`polly::say` is the worked example. Its `Cargo.toml` pulls `tokio` (`full`),
`aws-sdk-polly`, `aws-config`, and `soloud` — threads, a native HTTP stack, a
native C audio library — and it declares `imports: []`, doing all the work
itself. None of those build for `wasm32`, which is exactly why polly is
`executor: native` and browser-absent. A `Future::poll` refactor that keeps
those dependencies is still native-only.

To build the same function for wasm you remove them and route the two things a
guest cannot do — background work and I/O — through **non-blocking host
capabilities** the guest polls: a `synth` import (text → audio frames + a viseme
timeline) and an `audio` import (playback + playhead). The guest future then
holds no `tokio`/`aws`/`soloud`; it is pure coordination — poll the `synth`
channel, hand frames to `audio`, report `Status`. Because each host call is a
*non-blocking start/poll* and the async lives host-side, the guest never
suspends, so this works even on wasip1's synchronous, non-suspending ABI.

## Open question: how does a wasm module make a request?

polly needs the network (AWS Polly). A wasm guest **cannot open a socket** — and
today the arora browser build gives it nothing to reach for. `crates/arora-web`
builds on `arora` with default features off (its own words: *"a Tokio-free
synchronous runtime"*), pins `tokio` to `features = ["sync"]` (no `net`, no
`rt`), and carries no HTTP client and no `fetch` binding. So there is no path
from the guest to the wire.

The first thing to settle is what a request is *not*: it is **not a wasm
module**. A module runs in the sandbox precisely so it *cannot* touch the
outside; giving one network access would defeat the point. The request is always
a **host capability** — the abstraction lives host-side, and the guest only
calls it. Given that, the options:

- **An arora host capability (available now).** The host exposes a function the
  guest imports and the browser host implements over `fetch` (native: `reqwest`
  / the platform stack). Two grains:
  - a **generic `http` capability** — `request(url, headers, body)` — the
    reusable hook: *any* module that needs the network uses the one seam, and
    polly keeps its own request framing. Cost: `aws-sdk-polly`'s connector still
    will not build for wasm, so you either plug the SDK's `HttpClient` with a
    capability-backed connector or hand-build the signed REST call over `http`.
  - a **specific `synth` capability** — the host does the whole TTS request and
    streams frames back. Simplest for the browser (the SDK runs natively on the
    host), and it is exactly what `vizij-standalone` already does: `fetchVisemeData`
    is a JS `fetch`, the wasm side only consumes audio + visemes.

  Either way, expose it **non-blocking** (`start → handle`, `poll → Option`) so
  it composes with the poll-on-tick contract and does not block the engine on
  wasip1's synchronous host calls.

- **The standard `wasi:http` interface (the destination).** Rather than an
  arora-specific hook, a component imports `wasi:http/outgoing-handler`; the host
  provides it — wasmtime natively, the browser via a `fetch` shim (jco-style
  transpilation). Shaping the arora capability after `wasi:http` keeps it
  forward-compatible.

- **How wasip3 does it.** WASI 0.3 makes these interfaces **async**: `wasi:http`
  functions become `async func` over `future`/`stream`, so the guest simply
  `.await`s the request and **suspends natively** while the host drives it —
  wasmtime on the host, the browser via JSPI (Chrome yes, Firefox not yet). No
  custom hook and no manual poll: the request is an ordinary awaited call on a
  standard interface, and `Running`/pending becomes the language's own
  suspension. That is the end state; until the p3 toolchain and JSPI mature
  (study Stage 2), the non-blocking host capability above is the way, and it is
  the same shape one rung lower.

Recommendation: a **generic, non-blocking `http` host capability**, shaped after
`wasi:http`, is the reusable "built-in hook" — it unblocks polly-in-wasm and
every other module that needs the network, and graduates to real `wasi:http`
(host-async on p2 components, guest-async on p3) as the toolchain allows.

## The two ways to write the poll

You can write the state machine by hand — a plain synchronous function that,
each call, advances a step and returns `Status`:

```rust
fn step(run: RunId) -> Status {
    let s = state(run);                       // per-invocation state, by handle
    while let Some(item) = s.producer.try_next() {  // non-blocking channel poll
        s.consume(item);                      // hand off by function call
    }
    if s.producer.finished() && s.drained() { Status::Success } else { Status::Running }
}
```

Or, on a substrate that supports it, as an `async fn` the runtime polls — the
compiler writes the state machine, `.await` marks the yield points, and it
desugars to the same poll-on-tick. Prefer the **synchronous `step`** for
portable modules (it always works and needs no async substrate); reach for the
`async fn` form only where the control flow is branchy enough that a
hand-written state machine hurts, and only where the target supports it.

Either way, the interface the interpreter sees is unchanged: **a thing it
invokes once per tick that tells it `Running` or terminal.** That is the single
idea; everything else — where the work runs, how state is keyed, whether you
hand-write or generate the poll — hangs off it.
