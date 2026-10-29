# say-silent

The speech leaf of a behavior tree without a speaker: what a test, an offline
run or a device with no speaker registers in place of a speaking provider,
the tree unchanged. The crate declares the module `silent-say`, which a host
links and registers:

```rust
builder.with_host_module(HostModule::of::<say_silent::silent::Module>())
```

Every speech provider exports one function, `say(text, voice, viseme: &mut
String) -> Status`, under the same function and parameter ids, so a tree names
`say` without knowing who speaks:

| | Function id | `text` | `voice` | `viseme` |
|---|---|---|---|---|
| `say` | `77bf2798-e7ce-47c6-a45c-3c2e9ba1837d` | `881dc182-d4ba-4ea0-9e81-f4eddab6f669` | `f56ca142-db46-4c58-bc44-7896c4b54d5c` | `a1fbf58b-bf66-44a6-a503-9d9078ee5755` |

- `Running` while the sentence is said, `Success` once said and for as long as
  the leaf keeps being ticked (an episodic leaf: it pairs with an action in a
  `Parallel`), `Failure` when the voice cannot say it;
- sentences are keyed by their text: a new text forgets the finished ones, so
  a phase entered again speaks again;
- `viseme` is written with the mouth shape at the playhead (`sil` at rest).

This provider keeps a speaker's timing, counted in ticks (`TICKS_PER_CHARACTER`,
five a character), then logs the sentence ([`spoken`](src/lib.rs)) and
succeeds. The other providers declare the same ids:
[`say-piper`](../../examples/policy-modules/modules/say-piper) speaks with
Piper on board, and `naoqi-say`, in
[arora-aldebaran](https://github.com/semio-ai/arora-aldebaran)'s
`arora-hal-naoqi`, in a NAO or Pepper's own voice.
