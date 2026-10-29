# Long-running module functions

Most module functions return within one step. Some cannot: synthesizing
speech, a model's forward pass, a network request, a motion that plays out
over seconds. A device runs one synchronous step loop, and a call into a
module is part of a step, so such a function must never block. This page is
the contract for writing one.

## Return `Running`, be called again

A long-running function returns `Status::Running` and is called again on the
next step, until it returns a terminal status,
`Success` or `Failure`. The behavior that calls it — a behavior tree's leaf, a
node graph's node — calls it on every step for as long as it is `Running`.

```text
step 0:  say("hello") -> Running     // starts the work
step 1:  say("hello") -> Running     // still going
…
step N:  say("hello") -> Success     // done
```

`Running` is `Poll::Pending`: the function is a `Future` written by hand, the
step loop its executor, each call one `poll`, and a terminal status
`Poll::Ready`. Nothing about the loop changes; slow work is expressed as many
cheap calls.

A guest that blocks instead does not just stall the device: on wasmtime its
call is interrupted at the end of the step's budget, and the module refuses
every later call until it is reloaded
([time limits](../crates/arora-engine/readme.md#time-and-memory-a-guest-may-spend)).

## Why the loop drives the poll

The step loop has one owner of the device's state, takes no locks on its
control path, and evaluates each step in place. Awaiting inside a call would
either block that owner, stalling every other behavior of the step, or make
the step itself asynchronous. A WebAssembly guest cannot suspend mid-call
anyway: the `arora:module` world is synchronous. A synchronous poll driven
from the loop is how asynchronous work advances on a substrate that does not
suspend.

Work with yield points of its own — a request, a socket read, a wait on a
device — decomposes directly into "ready or not" polls. Work without them —
an inference that runs start to finish — runs elsewhere, and the poll only
asks whether it has finished. Both return `Running` until ready; only the
thing polled differs.

## State per invocation

The function is called again every step, so its progress lives between calls,
keyed by invocation, never in one process-wide value: two nodes calling the
same function would otherwise overwrite each other's progress. On its first
call a function starts the work and records it under a key; later calls look
it up and advance it; the terminal call reports and forgets it.

A call carries no run identity — only its arguments — so modules key their
runs by content: [`polly`](../modules/polly/readme.md) and
[`say-silent`](../modules/say-silent/readme.md) key an utterance by its text.
Two callers saying the same text at once share one run.

A function is not told when its caller stops calling it: a branch of a tree
left mid-run simply stops ticking its leaf. A module that holds resources for
a run releases them on its own terms — `say-piper` stops its audio once its
leaf has gone unticked for `IDLE_STOP`.

The speech leaves follow the episodic convention, so that they pair with an
action in a `Parallel`: a finished run keeps returning `Success` for as long
as it is called, and a new text starts a new run.

## Slow work runs off the step

The step must not wait, so the slow work — inference, I/O, playback — runs on
another thread, and the function polls a channel to it:

- [`polly`](../modules/polly/src/lib.rs) spawns the synthesis on a Tokio
  runtime and polls the task's `JoinHandle`, itself a `Future`, once per call
  with a no-op waker;
- [`say-piper`](../examples/policy-modules/modules/say-piper/src/lib.rs) sends
  each sentence to a worker thread that owns the voice, and each step takes
  what the worker produced with a non-blocking receive.

This is parallelism, which an async runtime alone does not give: awaiting
waits without blocking, but does not lend a second core to a forward pass.

Only native code can start that thread. A WebAssembly guest has no threads,
no runtime, and no sockets — the component executors link no `wasi:sockets` —
so `polly` and `say-piper` are native modules. A portable module keeps the
same contract and leaves the work to the host: it calls a host capability that
starts the work and returns a handle, and polls it each step.

## Writing the poll

A plain function advancing a state machine one step per call works on every
target and needs nothing from the substrate:

```rust
fn step(run: &mut Run) -> Status {
    while let Ok(item) = run.producer.try_recv() {     // never blocks
        run.consume(item);
    }
    if run.producer_done() && run.drained() { Status::Success } else { Status::Running }
}
```

An `async` block polled once per call is the same machine written by the
compiler, `.await` marking its yield points, and fits branchy control flow.
`polly` writes its synthesis so, runs it on a Tokio runtime, and polls its
`JoinHandle`.

## Open

- **Run identity.** The tree already passes parameters a node does not write
  literally — `children` to a control node, `_ret` for a non-`Status` return —
  and a run id would travel the same way, letting modules key runs by run
  rather than by content.
- **Background work and network for guests.** No host capability lets a
  guest start work off the step or make a request (see below).

[`async-modules-study.md`](async-modules-study.md) assesses how far the module
interface is from native async functions.

### Network access for a guest

A guest runs sandboxed: the `arora:module` world gives it no sockets, and
`arora-web` carries no HTTP client. A request is therefore a capability of the
host, which the guest calls; it is never something a module does by itself.
The options:

- **A generic `http` host capability** — `request(url, headers, body)` — one
  seam every module that needs the network shares, each keeping its own request
  framing. A module built on an SDK whose HTTP connector does not build for
  wasm (polly's `aws-sdk-polly`) plugs in a connector backed by the
  capability, or signs its REST call itself.
- **A task-specific capability** (a `synth` for speech): the host makes the
  whole request and hands the result back. Simplest for one use, reusable for
  none.
- **The standard `wasi:http` interface**: the component imports
  `wasi:http/outgoing-handler`, which wasmtime provides natively and a browser
  host would provide over `fetch`. WASI 0.3 makes it `async`, so a guest awaits
  the request and suspends while the host drives it (in the browser through
  JSPI), with no manual poll.

Whichever capability a host offers answers without blocking — `start` returns
a handle, `poll` returns the response once there is one — so a request
composes with [`Running`](#return-running-be-called-again). The recommended
first step is the generic capability, shaped after `wasi:http`, so that it
becomes the standard interface as the toolchain allows.
