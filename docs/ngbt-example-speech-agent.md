# Example: a speech-interactive agent, for the NGBT spectrum

Status: draft, for review. A spec for a new example in
[The NGBT spectrum](ngbt-spectrum.html).

The examples the NGBT spectrum compares today are small, and every node in them
returns its value in the tick it runs. This one is a robot talking with a
person: a speech-interactive agent harness on an embodied device. It mixes
stored content (clips, postures, filler lines), live input (face tracking,
voice activity), state with its own timing (emotions, cooldowns) and an async
pipeline (retrieval, memory, an LLM with tools, a judge, speech synthesis).

It stresses three things no earlier example does:

- **An async node graph.** Values arrive seconds after they are asked for, the
  LLM streams, and speech synthesis fans out per sentence. Each design has to
  say what such a node is: a tree leaf that returns `Running`, a detached node
  with a pending value, an action node in the graph.
- **Preemption without loss.** Listening preempts thinking and speaking, then
  can hand control back. A textbook behavior tree halts a preempted branch,
  which would lose the paused reply.
- **Layered blending across sources.** One channel is written by a stored clip,
  live tracking, an emotion and lipsync at once, and every handover fades.

## 1. The robot

A desktop robot: a body on a base, one arm, and a head on a neck whose face is
a screen with two eyes and a mouth. Nine channels:

| Part | Channels |
|---|---|
| head | `head/yaw`, `head/pitch` |
| eyes | `eyes/x`, `eyes/y` (both eyes gaze together), `eyes/lid` |
| mouth | `mouth/open`, `mouth/stretch`, `mouth/corners` (up and down) |
| arm | `arm/raise` |

## 2. Three parts

```
vad, asr, face ─▶ STATECHART ── states, emotion, gesture ──▶ MOTION GRAPH ─▶ 9 channels
                   │      ▲                                   ▲
     start, hold,  │      │ sentence, audio ready,            │
     resume, drop  ▼      │ playing, done                     │
                  SPEECH PIPELINE ──── visemes, playhead ─────┘
```

- **The statechart decides.** It holds the states and their transitions, and
  is what a design's behavior tree has to implement (§3).
- **The motion graph transforms and blends, every tick.** It turns the states,
  the live input and the assets into the nine channels (§4).
- **The speech pipeline transforms text and speech, asynchronously.** One run
  per turn, started, held, resumed and dropped by the statechart (§5).

What passes between them:

| From → to | What |
|---|---|
| inputs → statechart | voice activity (`vad`), recognized words (`asr`), face visible |
| inputs → motion graph | face position |
| statechart → motion graph | the states and when each started; the active emotions and their trigger times; the gesture |
| statechart → pipeline | start a run, hold, resume, drop; start a judge |
| pipeline → statechart | a sentence arrived (with its emotion and gesture), audio ready, playback started and ended, a tool call started, the reply ended, a verdict |
| pipeline → motion graph | the playing sentence's visemes, the audio playhead, whether speech is playing |

## 3. Statechart

Four regions run in parallel. *The round* is the conversation so far in this
interaction: utterances and replies since the last reset.

### 3.1 Conversation

| From | When | To | What happens |
|---|---|---|---|
| Idle | VAD on | Listening | |
| Thinking, Speaking | VAD on | Listening | The run is held. The audio pauses; a filler line stops for good |
| Listening | 1.5 s since VAD on, VAD off, no words | the state Listening came from | The run, and the audio, resume |
| Listening | VAD off with words, from Idle | Thinking | A run starts on the round and the words |
| Listening | VAD off with words, from Thinking or Speaking | Judging | The judge runs on the round, the reply so far and the new words |
| Judging | verdict `continue` | the state Listening came from | The run, and the audio, resume. The judge's follow-up sentences are added after the reply's last sentence |
| Judging | verdict `invalidated` | Thinking | The held run is dropped. A run starts on the round and the new words |
| Judging | VAD on | Listening | The judge is dropped |
| Thinking | sentence 1 arrives | Thinking (preparing) | Its emotion triggers; the thinking expression fades out |
| Thinking (preparing) | its audio starts | Speaking | |
| Speaking | the last sentence ends, follow-ups included | Idle | The turn is remembered |
| any | attention enters Nobody | Idle | The interaction resets: a run is dropped, the round is cleared |

- **Entering Thinking:** the thinking expression fades in and the eyes glance
  up for 1.2 s. Judging shows the thinking expression too.
- **A held run** keeps computing but shows nothing: no emotion, no playback,
  no filler line, no action tool, until Listening hands back (§5).
- Failures and timeouts are out of scope.

### 3.2 Attention

| From | When | To |
|---|---|---|
| Nobody | a face is visible | Present, or Engaged if the conversation is not Idle |
| Present | the conversation leaves Idle | Engaged |
| Engaged | the conversation returns to Idle | Present |
| Present, Engaged | the face is lost | Lost |
| Lost | the face is back | the state it left |
| Lost | 1.5 s | Nobody |

The gaze in each state:

- **Nobody:** a slow wander.
- **Present:** the person, with a glance away of 0.8 s, first 3.5 s after
  attending, then every 4 s.
- **Engaged:** the person, locked: no glances.
- **Lost:** the last target, held. Lost is a cooldown.

Attention triggers emotions:

- **Entering Lost** triggers *sad*.
- **The face coming back during Lost** triggers *surprised*.
- **Reaching Nobody** resets the interaction: the robot behaves as if it had
  never seen anyone, so the next face is a new person and triggers nothing.

### 3.3 Emotion

An emotion is a posture (§6) with an envelope. What triggers one:

- **A reply's first sentence:** when it arrives, so the face moves while its
  speech is synthesized.
- **A later sentence,** including a judge's follow-up: 0.6 s before it starts
  playing, so its fade ends at its first word.
- **Attention:** §3.2.

Each triggered emotion fades in over 0.6 s to its peak, holds for at most 5 s,
then fades out to neutral over 5 s.

A new trigger does not replace the emotions before it, it blends with them.
Each earlier emotion still showing fades out fast, over 0.6 s, but only once
it has reached its peak. An emotion nothing pushes out falls off naturally,
over the 5 s. For an emotion A triggered at 0 s and an emotion B at 0.3 s:

| Time | A | B |
|---|---|---|
| 0–0.3 s | fades in | |
| 0.3–0.6 s | fades in, ahead | fades in |
| 0.6–0.9 s | fades out | finishes fading in |
| 0.9–1.2 s | finishes fading out | at its peak |

The face shows the weighted average of the active postures, with neutral
filling in while their weights sum below 1. With `wᵢ` an emotion's envelope
between 0 and 1:

```
posture = (Σ wᵢ · Pᵢ  +  max(0, 1 − Σ wᵢ) · P_neutral) / max(1, Σ wᵢ)
```

### 3.4 Gesture

A gesture starts with the audio of the sentence that names it, and plays to
its end, even through a cut-in.

## 4. Motion graph

Every tick it reads the states and when each started, the active emotions,
the face position, the visemes and playhead and the assets, and writes the
nine channels:

- **Nothing jumps.** A gaze target, a posture or a viseme is a target the
  channels ease toward. A clip is keyframes, eased between keys and sampled
  every tick, laid over its channel with its own fades (§6).
- **Gaze target:** chosen by the attention state (§3.2).
- **Head:** follows the target with damping (half-life 0.25 s), plus the idle
  clip's sway and the posture's pitch. The head moves freely while it tracks.
- **Eyes:** follow the target about three times faster than the head
  (half-life 0.08 s against 0.25 s). The screen position is that followed
  target minus *this tick's* head, clamped to the screen, plus the thinking
  glance up. The eyes lead because the head is slower, and they never jump:
  a new target takes them about 0.3 s. They stay locked on the person
  whatever moves the head. Eyes that read the previous tick's head show the
  head's motion as gaze error.
- **Posture:** the blended emotions (§3.3), with the thinking expression over
  them.
- **Mouth:** the posture, with lipsync over it. The speech weight eases in and
  out over 80 ms; `open` and `stretch` blend toward the viseme at the
  playhead, and `corners` adds the viseme's offset to the posture's. The mouth
  mixes lipsync and the emotion fades at all times.
- **Lid:** the posture's lid × blink.
- **Arm:** the posture plus the idle clip's sway, overridden by a gesture clip.

Holds and fades:

| What | Timing |
|---|---|
| Head following the target | half-life 0.25 s |
| Eyes following the target | half-life 0.08 s |
| Face lost | the last target held for 1.5 s, then the wander |
| Glance away, while present | 0.8 s |
| Thinking glance up, eyes only | 1.2 s |
| Thinking expression | 0.3 s in and out |
| Emotion | 0.6 s in, at most 5 s held, 5 s out; 0.6 s out when blended away |
| Speech weight | 80 ms in and out |
| Gesture over the arm | 0.2 s in, 0.3 s out |
| Blink | 150 ms, every 2–5 s |

## 5. Speech pipeline

```
utterance ─┬─ rag 1 s ──────┐
           └─ recall 0.5 s ─┴─ prompt (+ tools) ─▶ llm ─┬─ tool call ─▶ tool ─▶ prompt + result ─▶ llm again
                                                        │      └─▶ filler line
                                                        └─ sentences {say, emotion, gesture?} ─▶ tts ─▶ play ─▶ playhead
round + new words ─ judge ⇢ verdict, then follow-up sentences ─▶ added after the reply
reply end ─ remember
```

| Stage | Kind | Timing | In → out |
|---|---|---|---|
| rag | async | 1 s | utterance, round → documents |
| recall | async | 0.5 s, beside rag | utterance, round → memories |
| prompt | pure | 2 ms | persona, tools, emotion, memories, documents, round, utterance and tool results → prompt |
| llm | async, streams | a tool call at +1 s, or sentences: the first at +2 s, the rest by +3 s | prompt → a tool call, or sentences `{say, emotion, gesture?}` |
| tool | async | per tool (§6) | call → result |
| filler | stored | its length | a tool call started → audio and visemes |
| judge | async, streams | the verdict at +1 s, follow-ups by +2 s | round, reply so far, new words → `continue` or `invalidated`, then follow-up sentences |
| tts | async | 1 s a sentence, one at a time, in order | sentence → audio and visemes |
| play | the audio's clock | the audio's length | audio → playhead; pausable |
| remember | async, fire and forget | | the turn → memory |

- **Tools:** each LLM round either calls a tool or starts the reply, with at
  most two tool rounds a run.
- **Fillers:** when a tool call starts in a run that is not held, a stored
  filler line plays through the lipsync path.
- **Held runs** may call info tools. Playback, filler lines, emotions and
  action tools wait until the run is handed back, so a dropped run leaves no
  effects.
- **The judge** decides whether the reply can continue after the user adds
  words, and writes what to add if it can.
- **Remember** records the turn when the reply ends or is dropped, partial
  replies included.
- **The mocks are deterministic.** The mock LLM's reply depends on which facts
  reached its prompt (§6), so a design that builds the prompt before the facts
  arrive says the wrong thing.

Long-running work follows Arora's contract: a function returns `Running` and
is polled each step ([async-functions.md](async-functions.md)).

## 6. Assets

The nodes refer to assets by name; the page shows their content in a panel
(§9).

```
postures    pitch  arm   lid  │ open stretch corners
  neutral    0     0     1    │  0    0       0
  happy     +.1   +.2   .85   │ .1   .4     +.8
  sad       -.25  -.1   .6    │  0  -.1     -.6
  surprised +.15  +.3   1.2   │ .5  -.3      0
  thinking  +.05   0    .9    │  0  -.2     -.1      the thinking expression

visemes     open stretch corners
  rest      .05   0       0      between words
  MBP        0   -.2      0      m, b, p: the lips pressed
  AA        .8   .1       0
  EE        .3   .8      +.2
  OO        .4  -.6       0

clips       keyframes, eased between keys, each laid over one channel
            idle   4 s loop    adds to pitch ±.03 and arm ±.05
            blink  0.16 s      multiplies the lid: 1 → .08 → .08 → 1
            nod    0.6 s       adds to pitch: 0 → -.15 → 0
            wave   1.5 s       overrides the arm, fading in 0.2 s and out 0.3 s: 0 → .9 → .6 → .9 → 0

prompts     persona  You are Pip, a small table robot. Speak in short sentences. Reply as JSON
                     lines {"say", "emotion": neutral|happy|sad|surprised, "gesture"?: wave|nod}.
                     Call a tool when you need live facts.
            turn     {persona} Tools: {tools}. You feel {emotion}. You remember: {memories}.
                     Facts: {docs}. So far: {round}. They said: "{utterance}"
            judge    So far: {round}. Facts: {docs}. You were saying: {reply}.
                     They just said: "{words}". Answer continue or invalidated; if continue,
                     add sentences that answer them, as JSON lines.

tools       weather(place, day)       info    1 s    → "Saturday at Riverside: sunny, 24 °C, UV index 8."
            calendar(day)             info    1 s
            set_reminder(text, when)  action  0.5 s
            skills: wave, nod                         a reply's gesture names one

knowledge   park  Riverside Park: open 8–20, shade by the lake.
            bus   Bus 12 runs every 20 minutes.        never retrieved

memory      Sam is planning a picnic on Saturday.
            Sam burns easily.

fillers     "Let me check."   when a tool call starts
            "One moment."     when a second tool call starts

tts         one viseme a vowel group (a → AA; e, i, y → EE; o, u → OO), 180 ms each;
            an MBP of 60 ms where m, b or p comes before a vowel group or ends a word;
            60 ms rest between words; a word's leading y is a consonant
```

The script and the mock outputs:

```
face        appears at 1.2 s, x -.4 → +.3 over 1.2–12 s, then +.3 → 0 over 15–20 s
            lost 14.0–15.0 s; leaves at 28.0 s
user        5.0–7.0    "Will it be nice for our picnic on Saturday?"
            8.6–9.6    "At Riverside Park, I mean."
            17.2–18.2  "Is there shade?"
            23.8–24.1  a cough: voice activity, no words

run 1       llm   → weather(here, Saturday)
            llm   → {happy} "Sunny all day, twenty-four degrees!"        dropped: never heard
judge 1     invalidated
run 2       llm   → weather(Riverside, Saturday)
            llm   → {happy} "Sunny at Riverside all day!"
                    {surprised, wave} "Oh, and you burn easily, so bring a hat!"
                    needs "UV index 8" and "burns easily" in the prompt; without them,
                    {neutral} "Have fun!"
judge 2     continue → {happy} "And yes, there's shade by the lake."
```

## 7. Scripted run

About 39 s at 30 Hz, ending as it began, so the example loops. The times are
rounded; the reference computes them from the TTS rule (§6).

1. **0–1.2 s, nobody.** Idle clip, wander, blinks, neutral.
2. **1.2 s, a face appears.** The eyes lead, the head follows. At 4.7 a glance
   away starts.
3. **5.0–7.0 s, the user asks.** The VAD cuts the glance short and the gaze
   locks.
   - 7.0: Thinking, run 1. The eyes glance up; rag and recall start.
   - 8.0: the prompt is built; the LLM starts.
4. **8.6–9.6 s, the user adds "At Riverside Park, I mean."** Run 1 is held.
   - 9.0: run 1's LLM calls `weather`. The call runs, but no filler line plays:
     the run is held.
   - 9.6: Judging.
   - 10.6: `invalidated`. Run 1 is dropped, its second LLM round in flight.
     Thinking, run 2; the eyes glance up again.
5. **10.6–15.6 s, run 2.**
   - 11.6: the prompt; the LLM.
   - 12.6: the LLM calls `weather`, and "Let me check." plays over the thinking
     expression.
   - 13.6: the prompt with the result; the LLM again.
   - 14.0: the face is lost. Sad fades in; the gaze holds.
   - 15.0: the face is back, inside the cooldown. Surprised fades in as sad
     fades out.
   - 15.6: sentence 1 arrives. Happy fades in as surprised fades out; the
     thinking expression fades out.
6. **16.6 s, "Sunny at Riverside all day!"**
   - 17.2–18.2: the user asks "Is there shade?" The audio pauses.
   - 18.2: Judging.
   - 19.2: `continue`. The sentence resumes from its pause; the judge's
     follow-up arrives at 20.2.
7. **19.9 s, surprised fades in under the sentence's last words.**
   - 20.5: "Oh, and you burn easily, so bring a hat!", with the wave.
   - 22.4: happy fades in.
   - 23.0: the follow-up, "And yes, there's shade by the lake."
8. **23.8 s, a cough.** The audio pauses; at 25.3, with no words, it resumes.
   - 26.7: the reply ends; the turn is remembered.
9. **28.0 s, the face leaves.** Sad fades in as happy fades out.
   - 29.5: Nobody. The interaction resets; the wander starts.
   - 28.6–33.6: sad holds; 33.6–38.6: it fades to neutral.

## 8. Measures

Beside each design's calls, visits, checks and modeled µs per tick:

- **Effects.** Per run: one rag and one recall; one prompt per LLM round; one
  tool call; one TTS per sentence. One judge per confirmed interruption. One
  filler line, from run 2.
- **Stale output.** No words, emotion, filler line or action tool from a held
  or dropped run.
- **Barge-in.** Ticks from the VAD to the audio pausing.
- **Resume.** A resumed sentence continues from its paused playhead.
- **Lipsync.** The mouth's lag behind the playhead, in ms.
- **Gaze lock.** The eyes' error on the person while the head moves.
- **Eye pops.** The largest eye move in one tick.
- **Emotion timing.** Each trigger at its time (§3.3), and no earlier emotion
  fading out before its peak.
- **Snaps.** The largest jump in one tick on any channel.
- **Ownership.** Ticks where two writers reach one channel with no rule.
- **False alarm.** The lock holds at least 1.5 s, then releases.
- **Reset.** After Nobody, the next face triggers no surprise and the round is
  empty.

## 9. The bench

A page of its own, [Speech Agent Bench](ngbt-speech-agent-bench.html), also
published as a claude.ai artifact: two implementations of the example side by
side, run in lockstep against one scripted world. It is one HTML file:
download it and open it in a browser.

- **Transport and timeline.** Play, scrub, step a tick, or jump to a moment of
  the run: a cut-in, the cough, the face lost.
- **Each side.** The robot, with a dashed ghost of the reference; live
  captions; the states; what it said; the service strip; the channels; the
  cost of each tick; and the scorecard. A failed check jumps to its moment.
- **How each design holds it.** One part at a time, the same part on both
  sides: the conversation, attention and reply run, or one of the seven motion
  groups. Each is drawn as a graph, as a live outline, or as its stored form.
  Nodes show what ran this tick, what answered from its cache, and each tree
  node's status. A node can be dragged to untangle a graph, with a reset per part.
- **Every design at a glance.** The whole run's numbers for all five designs.
- **The assets.** Every asset of §6, with the live ones following the left
  side: the postures showing, the prompts built, the memory written.
- **The implementations.** The reference, the spec as plain code, which
  every design is checked against; a naive foil that drops the reply on any
  cut-in, replaces emotions and reads the previous tick's head, for the
  scorecard to catch; and the five designs of §10.
- **Not there yet:** live controls (a face toggle and slider, buttons for the
  lines and the cough), and a Rust replay of the run, as for the Quori
  workloads.

## 10. How each design holds it

All five run one definition. The statechart is a tree and the reply run a
task run. The motion graph is 77 calls of `@arora/dataflow` (`damp`, `slew`,
`oscillator`, `if`, `read_record`, `blend_weighted_average`…) and of four
example modules (`@pip/motion`, `@pip/expression`, `@pip/lipsync`,
`@arora/animation`). It reads 13 store keys and publishes the nine channels.

None of the open pull requests says what an async node is, or how a paused
reply is held. Each design below therefore carries a proposal for those
parts. The proposals are the bench's, for each pull request's author to
confirm or replace.

The four tree designs (#396, #409, #404, #405) share these parts:

- **The reply run is a task run** the conversation spawns, with Arora's spawn
  and halt. It outlives the branch that started it. The tree writes
  `{reply/held}` to gate what it shows, and halts the run to drop it.
- **Its stages are module-call leaves** ticked at every tick, returning
  `Running` until done. This is Arora's async contract, and the way
  arora-dataflow-nodes places external calls and task runs.
- **The prompt is a node the llm leaf reads,** rebuilt when its arguments
  change. The dominant emotion is one of them, so a change of mood while the
  llm runs rebuilds it. The llm keeps the request it started.

| Design | Motion graph | When the motion sees the statechart | Prompt | Split into |
|---|---|---|---|---|
| #396 detached nodes (as #407 runs it) | 77 detached calls, pulled at their first read and cached by their arguments; sinks publish after the tree | The same tick | A detached node, run when its arguments change | Subtrees for the conversation and attention, a task run for the reply, detached nodes grouped by part |
| #409 full tree nodes | The same, each call also a full tree node | The same tick | The same | #396's file |
| #404 incremental tree | One node-graph shape per group, `PARALLEL[ leaf …, RUNNING ]`, leaves on change and bound to keys, after the statechart | The same tick | A leaf on change | One shape per group under the root `PARALLEL` |
| #405 macros, placed | One macro per group, placed at the root in a `WITH` before the tree | A tick late | A macro placed before llm | One macro per group, authored and placed |
| #257 BT in NG | Vizij graphs: control nodes for the conversation, attention and reply run; the motion eager | A tick late: a state the control writes reaches the motion through the store | A lazy node under llm, rebuilt at every tick llm is polled | One graph per region and per group, joined by keys |
| #257, prompt cached | #257, with #396's argument cache on the prompt alone, as #257's pull request suggests | A tick late | A lazy node with a cache, rebuilt when its arguments change | #257's graphs |

What the bench shows over the whole run:

- **#396, #409 and #404** produce the reference's nine channels at every
  tick.
- **#405 and #257** produce them a tick late wherever the statechart drives
  them. The mouth runs 33 ms behind the audio, under the 45 ms people notice,
  and the eyes jump a tick late when a face appears.
- **Modeled cost.** About 105 µs a tick for #396, #409, #404 and #405. The
  motion graph moves at every tick (the clock, the smoothing, the blinks), so
  caching saves about half of its 77 calls. #257 averages 451 µs a tick: it
  caches nothing, and rebuilds the prompt at every tick the llm is polled,
  168 two-millisecond builds against 7. With a cache on the prompt alone it
  averages 180 µs. The rest of the gap is its eager motion graph: 78 calls a
  tick against 42.
- **No design fails a check.** They differ in where the statechart's writes
  land in the tick, and in what runs again when nothing changed.

## 11. Extensions

The speech pipeline is the part with the most room to grow. Left for later:

- Backchannels while listening ("mm-hm" with a nod), a stored line and a clip.
- Streaming recognition, so retrieval and the prompt start before the user
  stops.
- Prosody from the TTS driving beat gestures.
- The user's voice and face triggering emotions.
- Several people: attention by speaker, a round per person.
- Echo suppression, so the robot's own audio does not trip the VAD.
- Failures and timeouts, with fallback lines.
- Summarizing the round into memory.

## 12. Open questions

1. **The async proposals of §10.** Each pull request's author to confirm or
   replace theirs: the reply run as a task run gated by `{reply/held}`, its
   stages as module-call leaves, and the prompt as a node.
2. **#257 with caching beyond the prompt.** The bench models a cache on the
   prompt alone (§10). Caching the motion graph too would bring #257 near
   #396; should it be a variant as well?
3. **Control timing in #257.** Only the motion's lag is modeled. Statechart
   transitions that cross ticks through latched state are not.
4. **A Rust replay of the run,** as for the Quori workloads, to replace the
   modeled costs with measured ones.
