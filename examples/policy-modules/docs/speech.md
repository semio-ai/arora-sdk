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

### Synthesis on a worker, playback in the tick

Synthesis is the long part — a model inference per sentence — and a tick
must not wait for it: the robot's policies share the tick. So `say-piper`
splits the work where it is long:

- **The worker thread synthesizes, and only that.** It owns the Piper voice
  and answers requests in turn: phonemize a text and synthesize its first
  sentence, or synthesize the next one. Each answer carries the sentence's
  samples and its phoneme timeline as mouth shapes.
- **The tick plays.** It owns the audio output and each sentence's queue on
  it. Each tick takes what the worker has produced and queues it, asks for
  the next sentence when less than 0.3 s of audio is queued ahead of the
  playhead, reads the playhead to write the mouth shape into `viseme`, and
  returns `Success` once the text is synthesized and played. It never waits
  on the worker.

The two share no data: requests go down one channel, sentences come back on
another, the samples moved rather than copied. Speech starts with the first
sentence rather than the whole text. A halt is silence: a leaf no longer
ticked asks for no further sentence, and the next `say` tick stops its
audio once it has gone 250 ms unticked. The voice's model load and first
inference happen at device start (`say_piper::warm_up`), not in a tick.

Measured on an Apple M1, the live device walking and kicking while it
speaks (the `en_US-lessac-medium` voice; a sentence takes about 78 ms to
synthesize):

| | median step | 99th percentile | worst step |
|---|---|---|---|
| silent voice (baseline) | 19.9 ms | 21.4 ms | 22.5 ms |
| Piper, synthesis in the tick | 19.8 ms | 22.6 ms | 113 ms |
| Piper, synthesis on the worker | 20.0 ms | 21.7 ms | 34–44 ms |

No `say` tick takes over 2 ms (a slower one is logged at debug level):
queuing a sentence is a move. What remains in the worst step is the
machine's, not the tick's — the inference's thread pool competing with the
control loop for cores — and the duck stays upright through every
announcement.

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
reuses Vizij's phoneme-to-shape table and chunk decoding; the split between
a synthesizing worker and a playing tick, and the latched success, are this
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
