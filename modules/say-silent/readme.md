# say-silent

The `Say` contract of behavior trees, without a speaker: what a test, an offline run or a
device with no speaker registers in place of a speaking provider, the tree unchanged.

The contract is `say(text, voice, viseme: &mut String) -> Status`, under fixed function
and parameter ids that every provider carries, so a tree names `Say` without knowing who
speaks:

- `Running` while the sentence is said, `Success` once said and for as long as the leaf
  keeps being ticked (an episodic leaf: it pairs with an action in a `Parallel`), `Failure`
  when the voice cannot say it;
- sentences are keyed by their text: a new text forgets the finished ones, so a phase
  entered again speaks again;
- `viseme` carries the mouth shape at the playhead (`sil` at rest).

| | Function id | `text` | `voice` | `viseme` |
|---|---|---|---|---|
| `say` | `77bf2798-e7ce-47c6-a45c-3c2e9ba1837d` | `881dc182-d4ba-4ea0-9e81-f4eddab6f669` | `f56ca142-db46-4c58-bc44-7896c4b54d5c` | `a1fbf58b-bf66-44a6-a503-9d9078ee5755` |

This provider keeps a speaker's timing, counted in ticks (five a character), then logs the
sentence ([`spoken`](src/lib.rs)) and succeeds. Other providers: `arora-hal-naoqi`'s
`naoqi-say` (a NAO or Pepper's own voice, in
[arora-aldebaran](https://github.com/semio-ai/arora-aldebaran)), and Piper on board in `examples/policy-modules`.
