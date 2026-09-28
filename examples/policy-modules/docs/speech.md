# Speaking from the behavior tree

The robot announces each phase of its behavior out loud — "Ready to go.",
"Watch my kick!", "Time to sit down." — through a `Say` leaf in the `interactive`
tree. This page explains how a speech leaf fits a tree that also runs
control policies, what the speech module is, and where it should live.

## The leaf

`say(text, voice, viseme: &mut String) -> Status` is Vizij's speech
contract: fixed function and parameter ids, one signature, several
providers. A call starts an utterance and returns `Running`; the tree
re-ticks it, and each tick polls the utterance and writes the mouth shape
at the audio playhead into `viseme`; it returns `Success` when the audio
ends and `Failure` when synthesis or playback fails. Stopping the ticks
stops the audio within 250 ms — a halt is silence.

Two providers, registered one at a time (`policy-device --speech`):

| Provider | Crate | What it does |
|---|---|---|
| `cloud` (default) | `modules/say` | the Vizij TTS cloud function (AWS Polly behind HTTPS, no credentials in the app), played on the host's audio output through `rodio` |
| `silent` | `modules/say-silent` | takes a speaker's time (five ticks a character), then logs the sentence: tests, offline runs |

Both are host modules, whatever executor the policies use: speech needs the
network and a speaker, which a wasm guest has no access to. The split falls
where it should — pure computation (the networks) ships as a portable
guest, I/O-bound providers are the host's.

## The voice beside the behavior

A tree that speaks while it balances has to meet three constraints:

- **The policy must keep running while the robot speaks.** A sentence lasts
  a second or more; a leg left without its policy for that long falls. So
  speech is never a step *before* an action in a sequence.
- **A phase is announced once.** The engine's `Parallel` ticks every child
  every tick, so a `Say` inside one restarts as soon as it succeeds, and a
  `SequenceStar` forgets its place only when it finishes — a sitting branch
  that never finishes would never speak again.
- **A short phase is still announced in full.** A kick lasts 0.5 s; the
  cloud voice needs about as long to synthesize "Watch my kick!" and a
  second and a half to say it (the first sentence of a run waits a few
  more seconds for the cloud function to wake). A voice that followed the
  phase would lose every sentence longer than its phase.

The `interactive` tree meets them with a voice that runs in parallel with
the behavior, takes the phase's sentence when it is idle, and says it to the
end:

```xml
<Parallel>
  <Fallback>                                   <!-- the behavior -->
    <Sequence>
      <Equals value="{command.behavior}" expected="kick_left" />
      <Assign value="Kick!" target="{speech.text}" />
      <Skill skill="kick_left" … />
      <Assign value="walk" target="{command.behavior}" />
    </Sequence>
    …
  </Fallback>
  <Fallback>                                   <!-- the voice -->
    <Sequence>                                 <!-- idle: take what is new -->
      <Equals value="{speech.saying}" expected="" />
      <Fallback>
        <Equals value="{speech.text}" expected="{speech.said}" />
        <Assign value="{speech.text}" target="{speech.saying}" />
      </Fallback>
    </Sequence>
    <Sequence>                                 <!-- busy: say it to the end -->
      <Fallback>
        <Say text="{speech.saying}" voice="{speech.voice}" viseme="{speech.viseme}" />
        <Succeed />
      </Fallback>
      <Assign value="{speech.saying}" target="{speech.said}" />
      <Assign value="" target="{speech.saying}" />
    </Sequence>
  </Fallback>
</Parallel>
```

Each branch writes its phase's sentence to `speech.text` every tick —
idempotent, so it costs nothing. When the voice is idle (`speech.saying`
empty) and `speech.text` differs from `speech.said`, it takes the sentence
into `speech.saying`; from the next tick it says that sentence to the end,
records it as said and is idle again. What follows from the shape:

- the policies never stop for speech;
- a sentence is never cut by the next phase: the kick is announced in full
  while the robot is already walking again;
- the voice says the latest phase's sentence when it frees up, not a
  backlog: a phase that starts and ends while another sentence plays is
  not announced;
- a sentence that cannot be said (no network) is skipped, not retried
  every tick;
- the same sentence twice in a row is said once — a phase that must repeat
  its announcement writes a different sentence in between (walking says
  "Ready to go." between two kicks);
- `speech.voice` is a key: an operator changes the voice live (any AWS
  Polly voice the endpoint accepts);
- the tree still decides when a sentence must stop: a branch that ends the
  `Say` (a stop command, a fall that must be announced at once) halts it,
  and the provider goes quiet within 250 ms;
- `speech.viseme` carries the mouth shape at the playhead: what a face — or
  the Microduck's mouth joint, which this simulation does not model — would
  follow.

## Where Say should live

The contract and the cloud provider live in vizij-rs today
(`vizij-arora-host::skills` holds the ids, `vizij-arora-behavior::speech`
the signature and Vizij's lip-sync fragment, `vizij-arora-tts` the cloud
provider, `vizij-piper` a local one). A robot that is not a Vizij face cannot
depend on them without Vizij's face and node-graph crates, so this example
carries a copy: `modules/say` is `vizij-arora-tts` 4.0.0's native producer
(synthesis, `rodio` playback, the halt pulse, Polly's viseme table) behind
the same ids, declared with `#[arora_module::module]`, which gives the leaf a
typed `Status` return and `&mut String` out-parameter.

Taking Say out of Vizij splits it along the lines it already has:

| Piece | Home | Depends on |
|---|---|---|
| the contract: ids, signature, the rest shape, the viseme vocabulary | a small crate of its own, next to the Arora crates | `arora-types`, `arora-behavior` |
| each provider (cloud, Piper, silent) | a sibling crate per provider, one module id each | the contract |
| the lip-sync fragment that turns `viseme` into a face's shapes | Vizij | the contract |

A device then picks its provider by a feature or an option, as this one
does, and a tree names `Say` without knowing which. Two things the move
should settle, which the copy inherits:

- **Run identity.** The module ABI hands a call no run id, so utterances are
  keyed by their text and voice: two branches saying the same sentence at
  once share one utterance. The SDK's async-functions design records the
  run-id question.
- **One symbol per function id per crate.** `#[arora_module::module]` exports
  a symbol named after each function id, so two providers of one contract
  cannot share a crate — the shape the table above takes anyway.
