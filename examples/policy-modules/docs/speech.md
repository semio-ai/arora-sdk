# Speaking from the behavior tree

The robot announces each phase of its behavior out loud, on board — "Ready
to go.", "Watch my kick!", "Time to sit down." — through a `Say` leaf that
runs beside the phase's policy in the `interactive` tree. This page explains
how the leaf works, how it fits a tree that also runs control policies, and
where it should live.

## The leaf

`say(text, voice, viseme: &mut String) -> Status` is Vizij's speech
contract: fixed function and parameter ids, one signature, several
providers. `Running` while the sentence is synthesized and played;
`Success` when it has played, and for as long as the leaf keeps being
ticked afterwards — the episodic-leaf convention the policy leaves follow
too; `Failure` when the voice or the audio output fails. `viseme` carries
the mouth shape at the audio playhead (`PP`, `aa`, … and `sil` at rest).

Two providers, registered one at a time (`policy-device --speech`):

| Provider | Crate | What it does |
|---|---|---|
| `piper` (the `piper` feature) | `modules/say-piper` | Piper synthesizes on board, the host's audio output plays: no network, no credentials |
| `silent` | `modules/say-silent` | a speaker's timing counted in ticks (five a character), each sentence logged: tests, builds without Piper |

Both are host modules, whatever executor the policies use: a speaker is the
host's, not a portable guest's.

### An iterative Say

`say-piper` does its work in the ticks that call it; nothing runs on its
behalf between them but the audio device.

1. The first tick phonemizes the text (`piper_synthesize_start`, espeak-ng).
2. Each tick where less than 0.3 s of audio is queued ahead of the playhead
   synthesizes the next sentence (`piper_synthesize_next`: one model
   inference) and queues its samples, and its phoneme timeline, on the
   utterance's sink.
3. The audio output plays the queue on its device thread; each tick reads
   the playhead and writes the mouth shape there into `viseme`.
4. The tick that finds the text fully synthesized and the queue played out
   returns `Success`.

A tick does at most one sentence's inference, and speech starts as soon as
the first sentence is ready rather than when the whole text is. A halt is
silence: a `say` the tree stops ticking for 250 ms is stopped by the audio
thread. The voice's model load and first inference happen at device start
(`say_piper::warm_up`), not in a tick.

What a sentence's inference costs the control loop, on an Apple M1 (the
`en_US-lessac-medium` voice, about a second of audio per sentence): 78 ms
for the sentence, so the tick that synthesizes it stalls the 50 Hz loop for
four control periods. On the live device, steps took a median 19.8 ms, a
99th percentile of 22.6 ms and a worst of 113 ms, and the duck stayed
upright through every announcement. Should a policy need a steadier loop,
the step that runs the inference is the one to hand to a worker, the tick
still requesting each sentence and collecting it.

## Speech beside the policy

A sentence lasts a second or more, and a leg left without its policy that
long falls: speech is never a step before an action. Each branch of the
`interactive` tree runs its policy and its sentence in a `Parallel`:

```xml
<Sequence>
  <Equals value="{command.behavior}" expected="kick_left" />
  <Parallel>
    <Skill skill="kick_left" … />
    <Fallback>
      <Say text="Watch my kick!" voice="" viseme="{speech.viseme}" />
      <Succeed />
    </Fallback>
  </Parallel>
  <Assign value="walk" target="{command.behavior}" />
</Sequence>
```

The engine's `Parallel` ticks every child every tick and succeeds when all
have. With the Say latching its `Success`, that composition gives:

- **the policy never stops for speech**: both children are ticked every tick;
- **once per entry into the branch**: a finished Say keeps succeeding while
  the branch runs (a seated robot does not repeat itself), and a new text
  forgets finished sentences, so the phase speaks again next time;
- **a phase ends when its policy and its sentence both have**: the kick's
  network keeps running, holding the robot, until "Watch my kick!" has
  played, then the branch hands back to walking — for a continuous policy
  (walking, sitting, standing) the branch never ends, and the sentence is
  said once;
- **leaving a branch halts its sentence**: the next phase's Say takes the
  synthesizer and the old sink stops within the halt bound;
- **speech is best effort**: the `Fallback` with `Succeed` keeps a voice
  that cannot speak from failing the `Parallel`, which would send the
  fallback to the next branch and tick a second policy in the same tick.

## Where Say should live

The contract and its providers live in vizij-rs today:
`vizij-arora-host::skills` holds the ids, `vizij-arora-behavior::speech` the
signature and Vizij's lip-sync fragment, `vizij-arora-tts` the cloud
provider, `vizij-piper` Piper's provisioning, and the Vizij app's
`tts_piper` module the Piper provider. A robot that is not a Vizij face
cannot depend on them without Vizij's face and node-graph crates, so this
example carries copies: `modules/say-piper/build.rs` is `vizij-piper`'s
build script verbatim (it shares that crate's cache), and the provider
reuses Vizij's phoneme-to-shape table and chunk decoding; the iterative
driving, the latched success and the halt by the audio thread are this
example's.

Taking Say out of Vizij splits it along the lines it already has:

| Piece | Home | Depends on |
|---|---|---|
| the contract: ids, signature, the rest shape, the viseme vocabulary | a small crate of its own, next to the Arora crates | `arora-types`, `arora-behavior` |
| each provider (Piper, cloud, silent) | a sibling crate per provider, one module id each | the contract |
| Piper's provisioning (libpiper, espeak-ng data, the aligned voice) | the Piper provider's crate, GPLv3 | — |
| the lip-sync fragment that turns `viseme` into a face's shapes | Vizij | the contract |

A device then picks its provider by a feature or an option, as this one
does, and a tree names `Say` without knowing which. Settled by the move,
inherited by the copy:

- **Run identity.** The module ABI hands a call no run id, so utterances are
  keyed by their text: two branches saying the same sentence at once share
  one utterance, and a provider tells a re-entered phase from a continued
  one only by a new text in between or the halt bound. The SDK's
  async-functions design records the run-id question.
- **One provider per crate.** `#[arora_module::module]` exports a symbol
  named after each function id, so two providers of one contract cannot
  share a crate — the table's shape anyway.
- **Licensing.** libpiper and espeak-ng are GPLv3: the Piper provider is a
  GPL crate, and a binary linking it is GPL-affected; here it is behind the
  device's opt-in `piper` feature and out of the workspace's default
  members.
