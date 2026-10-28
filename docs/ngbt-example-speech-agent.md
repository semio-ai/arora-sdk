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

About 39 s on a 100 Hz clock, ending as it began, so the example loops. The
times are rounded; the reference computes them from the TTS rule (§6).

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

- **Work.** At each tick, every implementation counts its node ticks (every
  node the tick steps through, polls and what they read included), its
  executions (what runs) and its checks (what it reads or compares to
  decide). They are split by area: the statechart, the async polls, the prompt
  and the motion graph. The reference counts its own with tracer calls, and
  the designs count theirs by their rules (§10). The modeled µs of each tick
  comes from these counts.
- **Waiting.** One second waiting on the llm: what each build does for the
  wait over its 100 ticks. A waiting leaf is ticked when its call starts and
  when it completes, and in between is woken on completion, not polled.
- **Each build at its best.** Each build is measured with every optimization
  its framework can take, each checked to leave its channels exact (§10).
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
- **Each side.** A frame offset, which starts at the design's lag, shows a
  design that runs a tick late at the same moment of its execution as the other
  side; the reference's ghost stays on the timeline. The robot, with a dashed
  ghost of the reference, and Sam beside it: away, in view, speaking, or turned
  away while the face is lost; live
  captions; the states, and every leaf waiting, polled or not; what it said; the service strip; the channels; the
  cost of each tick, charted as µs, node ticks, executions or checks, with
  this tick's work by area; and the scorecard. A failed check jumps to its
  moment.
- **How each design holds it.** One part at a time, the same part on both
  sides: the conversation, attention and reply run, or one of the seven motion
  groups; or Full, every part at once, the three trees above the whole motion
  graph. Each is drawn as a graph, as a live outline, or as its stored form.
  Every stored form uses one notation: `{key}` a store key, `{.name}` a
  tree-local cell, `@name` a node's output, `name:` a node's name, a key or
  cell before `=` what a call's output is bound to, `#name` a macro, and
  `// …` a comment. Every tree node has a name, as its author would write
  it (`listen_branch`, `is_listening`), and a function is named by its
  module (`agent.llm` for `@pip/agent`'s `llm`). A motion group is listed
  output by output: the nodes each output needs, under a comment naming it,
  and a comment for what it reads from an output above. A Vizij graph file
  holds flat lists of nodes and edges, so #257's stored form writes the
  edges as indentation, each node under the node it is wired into. A node
  wired into several sits under the first, and a comment names it under the
  others. The other designs cannot nest: in a tree file, indentation is the
  tree, and in a group of leaves their order is the order they run.
  A node's colour is what the tick did with it, by the design's rule: blue,
  re-evaluated or ticked; amber, visited to collect its data and kept (a
  detached node comparing its arguments, a tree node whose cached status its
  parent checked); none, not visited, fainter when not needed. Edges between
  two blue nodes are blue, edges into or out of a visit amber. A tree node's
  border is its status. A node can be dragged to untangle a graph, with a reset per part.
- **The inspector and the watch.** Select a node in any view, or a line that
  names one: each side's inspector shows what its design holds for the node's
  inputs and output at this tick, with where it is kept and its state (ran,
  kept, stale, a tick late, not what the node ran with, or not kept). Any bool,
  state or number among them can be watched, up to six, on a plot under the
  views, as each side's design holds it: bools on 0 and 1, states on the
  steps of the longest watched list, numbers from ±1 up as needed. A dashed
  line is a value that is stale, a tick late, or not what the node ran with;
  a gap, one the design does not keep. Text is not plotted.
- **Every implementation at a glance.** The whole run's numbers for the
  reference, the foil and the designs: each one's average tick and its work.
- **Inspecting values, at a glance.** What an inspector finds in each build
  over the run (§10, Inspecting values).
- **The assets.** Every asset of §6, with the live ones following the left
  side: the postures showing, the prompts built, the memory written.
- **The implementations.** The reference, the spec as bespoke code, which
  every design is checked against; a naive foil, the reference's code with
  three rules broken for the scorecard to catch (it drops the reply on any
  cut-in, replaces emotions and reads the previous tick's head); and the five
  designs of §10.
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
- **Its stages are module-call leaves,** returning `Running` until done. This
  is Arora's async contract, and the way arora-dataflow-nodes places external
  calls and task runs.
- **The prompt is a node the llm leaf reads** when its call starts, cached by
  its arguments. The llm keeps the request it started.

| Design | Motion graph | When the motion sees the statechart | Prompt | Split into |
|---|---|---|---|---|
| #396 detached nodes (as #407 runs it) | 77 detached calls, pulled at their first read and cached by their arguments; sinks publish after the tree | The same tick | A detached node, run when its arguments change | Subtrees for the conversation and attention, a task run for the reply, detached nodes grouped by part |
| #409 full tree nodes | The same, each call also a full tree node | The same tick | The same | #396's file |
| #404 incremental tree | One node-graph shape per group, `PARALLEL[ leaf …, RUNNING ]`, leaves on change and bound to keys, after the statechart | The same tick | A leaf on change | One shape per group under the root `PARALLEL` |
| #405 macros, placed | One macro per group, placed at the root in a `WITH` before the tree | A tick late | A macro placed before llm | One macro per group, authored and placed |
| #257 BT in NG | Vizij graphs: control nodes for the conversation, attention and reply run; the motion one graph whose nodes keep their outputs until a node they read changes and marks them | A tick late: a state the control writes reaches the motion through the store | A lazy node under llm, run when its arguments change | One graph per region, joined to the motion by keys; the motion one graph, shown a group at a time, since a store hop between its groups would put the eyes a tick behind the head |

#257's pull request was closed on 21 October in favor of #396.

### How the work is counted

The bench runs on one clock at 100 Hz, and nothing in it is async: a wait is
a leaf that answers `Running` until its work is done. A waiting leaf reads its
arguments when its call starts and stays clean until the call completes, so a
second of waiting on the llm is 100 checks of its clean branch, one a tick,
and nothing else.

- **The reference** counts with tracer calls in its own code, in the motion
  graph's units: a function of the graph that its code computes is a node tick
  and an execution. Where the graph has a selection node (`if`), the code takes
  a branch instead, which computes nothing. Each region it steps (the
  conversation, the attention, the emotions) is a node tick, and each guard a
  check. It polls nothing: its event loop checks one queue a tick for every
  wait. A call's start and its result run once each, and the prompt is built
  once per call.
- **The designs** count by their rules:
  - The statechart is the dirty nodes its tick reaches, with a check for each
    clean child.
  - A call's start and its result are a module call each. A store key is a
    check. A node is a node tick that answers from its cache (detached: a
    check for each argument; versioned cells: one check) or runs again.
  - The motion graph follows each design's rule in the table above, with the
    optimizations below: in #396, #409 and #257 a node that recomputes to a
    new value marks its readers, and only marked nodes are visited; #404's and
    #405's cells are versioned, and only the leaves whose inputs changed are
    ticked.
- **Modeled µs.** The designs pay the NGBT spectrum's per-operation rates,
  plus 2 µs of work a call and 2 ms a prompt build. A poll that answers
  `Running` does no work and pays only its module call. The reference pays for
  its own work and nothing else.

### The optimizations each build runs with

Each build runs with every optimization its framework can take that lowers its
modeled cost, and with invalidation on #396 and #409, which does not (below).
Each one keeps the build's channels exact; the bench checks that at every
tick.

| Optimization | What it does | From | Builds |
|---|---|---|---|
| A cache on every node | A node whose inputs did not change keeps its output | the other designs, which all cache | #257 |
| Invalidation instead of comparison | A node that recomputes to a new value marks its readers; a tick runs only the marked nodes, and a clean one is neither visited nor compared | the bench, as #404's cells do | #257, #396, #409 |
| An incremental control tick | A control node whose children are clean answers from its cache; only the marked branches are ticked | the bench, by #404's rules | #257 |
| A dirty queue and a store subscription | A clean leaf is not checked; a store key is observed only when something writes it | #404, §5 of its proposal | #404, #405, and the trees of #396 and #409 |
| The tree on #404's executor | A clean subtree of the statechart answers from its cache | #404, which any tree can opt into | #396, #409 |
| Sinks publish on change | A sink writes its key when its value changed | #404's comparison with #396 | #396, #409 |
| Lazy conditionals | `if`, `and`, `or`, `hold` and a blend at weight 0 pull only the inputs their result needs | the bench | all |
| Time horizons | A clip, an envelope or the thinking glance says when its value next changes, and its node does not run until then | the bench | all |
| Latched arguments | A waiting leaf reads its arguments once, when its call starts, so the prompt is built once a call | the bench | all |
| Native polls | A waiting leaf checks its call's handle without calling its module | the bench | all |
| Wake on completion | A waiting leaf stays clean until its call completes, so its branch is not ticked while it waits | the bench | the trees on #404's executor, and #257 with its incremental control tick |

**Invalidation on #396 and #409 costs 0.24 µs more a tick, and is kept.** A
detached node is held by no node and has no list of its readers: pulled, it
checks its arguments to know its cached value still holds, and they are
detached nodes too, so every needed node is visited and compared at every
tick. With invalidation the executor builds each node's readers when the file
loads, and a node that recomputes to a new value marks them. That halves their
node ticks, from 64 to 32 a tick, and cuts their checks from 104 to 60; they
then visit what #257 visits, node for node. In µs it costs more, since a value
comparison here (9.5 ns) costs less than the version write that marks a reader
(38 ns), and 28 of the 75 needed nodes change at every tick. For #257 it saves
its input and output nodes' visits, and pays.

Two candidates were tried and dropped. Versions instead of value comparison
(#396's proposal, §9), tried before invalidation, cost 0.7 µs more: nearly
every value here is a number, cheaper to compare than to version. #405's
`WITH[ PARALLEL[ leaf × N ], RUN ]` shape for #404's groups cost 0.2 µs more:
every group but the mouth's changes at every tick, so few answer settled.

Lazy conditionals skip the most work. While someone is there, the wander's
oscillators do not run, and while Pip is engaged, neither does the glance
timer. Time horizons skip the next most: the blink clip between blinks, the
wave when no gesture plays, the emotions' weights while they hold, the thinking
glance outside its 1.2 s.

### The bespoke reference

The reference is the spec written bespoke:

- **An event loop.** The voice, the words, every call's result and every
  deadline (the false alarm's 1.5 s, the lost face's cooldown, an emotion's hold
  and fade, the audio's end) come through one queue, checked once a tick.
- **Motion computed on change.** A value is computed when what it follows from
  changed. A follower at its target stays put. The posture is blended when an
  emotion fades or the thinking expression moves, and the mouth when the
  viseme or the speech weight does.
- **The script's schedule.** It knows when a blink, the wave, an emotion's
  fade, the thinking glance or the glance away next changes. It also knows when
  the viseme moves: only in the 50 ms crossfade at the start of each segment of
  the track.

It costs 55 µs a tick and computes 26.6 of the graph's functions a tick.

At the graph's granularity, 28.3 functions a tick are needed and change value.
That is the floor for an exact implementation that computes every one of them,
and the designs run 29.0. The bespoke reference runs fewer, 26.6:
it takes a branch where the graph has a selection node, skips the arm's blend
while no wave plays, and knows two schedules a design does not (the glance
away and the viseme's crossfades).

### What the bench shows over the whole run

- **#396, #409 and #404** produce the reference's nine channels at every
  tick.
- **#405 and #257** produce them a tick late wherever the statechart drives
  them. At 100 Hz the mouth runs 10 ms behind the audio, under the 45 ms
  people notice, and the eyes move a tick late when a face appears.
- **Work a tick,** on average, with one second waiting on the llm as node
  ticks · executions · checks:

  | Implementation | µs | Node ticks | Executions | Checks | 1 s on the llm | Prompt builds |
  |---|---|---|---|---|---|---|
  | Reference | **55** | 28 | 27 | 18 | 0 · 0 · 100 | 4 |
  | #396 detached nodes | 65.7 | 32 | 29 | 60 | 0 · 0 · 100 | 4 |
  | #409 full tree nodes | 66.4 | 32 | 29 | 60 | 0 · 0 · 100 | 4 |
  | #404 incremental tree | 66 | 46 | 29 | 64 | 0 · 0 · 100 | 4 |
  | #405 macros, placed | 66 | 43 | 29 | 64 | 0 · 0 · 100 | 4 |
  | #257 BT in NG | 66 | 40 | 29 | 60 | 0 · 0 · 100 | 4 |

- **The designs end within 0.7 µs of each other.** What is left is
  the 29 functions that change a tick, at 2 µs each, and each framework's
  bookkeeping. #396, #409 and #257, with invalidation, visit only what changed:
  the same nodes at nearly every tick, and the same 60 checks. #396 and #409
  tick the fewest nodes (32 a tick); #257 also visits its graph's input and
  output nodes (40). #404 and #405 tick 46 and 43, with 64 checks.
- **The bespoke reference is the cheapest,** 10.4 µs a tick under the best
  design. It makes 2.3 fewer calls a tick (4.6 µs): the selections it takes as
  branches, the arm's blend it skips while no wave plays, and the two
  schedules. The other 5.8 µs is bookkeeping a framework pays and it does not.
- **The prompt.** Every design builds it once per call, four times, as the
  reference does.
- **No build fails a check.**

### Inspecting values

A debugger's watch on a node: its inputs and its output at every tick. The
designs all map to the same kind of view, so what tells them apart is what
each keeps for that view to read. Read from each pull request's code (paths
under `crates/arora-behavior-tree/src/`, and vizij-rs
`crates/node-graph/vizij-graph-core/src/` for #257), and measured on the
motion graph over the run:

| Implementation | Outputs kept | Stale among them | Inputs show what the node ran with | Store keys a tick apart | Statuses | What an inspector reaches |
|---|---|---|---|---|---|---|
| Reference | 45% | — | — | 0% | none: no tree | nothing it names: instrument the code |
| #396 detached nodes | 100% | 13% | 98% | 0% | kept, private | `detached_output`; `observe_detached` |
| #409 full tree nodes | 100% | 13% | 98% | 0% | kept; each call's `detached_status` | as #396, with a status and a trace line per call |
| #404 incremental tree | 100% | 13% | 87% | 0% | kept, private | nothing outside the tree |
| #405 macros, placed | 100% | 13% | 92% | 9.2% | kept, private | less than #404 for placed nodes |
| #257 BT in NG | 100% | 13% | 92% | 9.2% | the `out` port, typed `Status` | `eval_all`, `get_outputs_delta` |

- **No design passes node values by publish and subscribe.** Each keeps them
  in a cache, a cell or a port, for the next tick to read. The store is the
  only push channel (`subscribe` in `arora-types/src/data/store.rs:322-345`,
  and the bridge's `subscribe` and `values_changed`), and it carries store
  keys, never a node's inputs or outputs. A tree-local cell reaches no observer
  (`variable.rs:109-115`).
- **Every build holds the same stale values,** 13% of its node outputs: they
  take the same laziness, so the same nodes go unread. They differ in what they
  keep beside the output, and in whether anything outside the executor can
  tell a stale value from a current one. No build exposes that today; #257's
  code sidesteps it by keeping nothing for a node it did not tick.
- **#396 and #409 keep the most.** A detached node's cache holds its last call:
  the outputs, and the arguments it ran with, which it compares and lends to
  the call (`detached.rs:186-199, 1000-1025, 1063-1066`). An input row shows
  what the node ran with at every tick after its first call, older than what
  feeds it now at 19% of input-ticks. `detached_output` reads the outputs. The
  arguments and the cache's tick are private: an accessor for each would give
  an inspector the whole call, and staleness by comparing the arguments with
  their sources. `observe_detached` keeps a node current by running it at
  every tick (`behavior_tree.rs:92-98`), which advances a stateful node on a
  branch not taken: watching that way changes what runs. #409 adds a status
  per call, `detached_status`, which is none before the first call and after
  a failure, so a failed call shows, and a trace line per real call
  (`behavior_tree.rs:98, 1295` on #409).
- **#404 keeps statuses and cells, but no values of a node's own.** A leaf's
  output is the cell it is bound to, and its inputs are the cells it reads, as
  they are now (`behavior_tree.rs:1398-1406`). Those are what it ran with only
  while the leaf is clean: an input row shows a newer value at 13% of
  input-ticks, and what the node ran with is gone. A cell counts versions only
  while it has readers. The dirty bit and the statuses are private
  (`incremental.rs:49-58`), and nothing reaches an observer outside the tree.
- **#405 shows less than #404.** A placed node is an `_on_change` leaf with a
  private port, which `observe_detached` and `detached_outputs` no longer reach
  (`docs/proposal-placing-detached-nodes.md:264-270` on #405). Unplaced nodes
  keep #396's cache, so one file has two regimes. Its groups run before the
  tree, so at 9.2% of ticks the graph and the store hold different values of
  a key.
- **#257 has the surface an inspector wants, and keeps no inputs.** Every port
  is typed and shaped, a control node's status is its `out` port, and the wasm
  `eval_all` returns every port, with `get_outputs_delta` the changes since a
  token (`vizij-graph-wasm/src/lib.rs:1079-1127`). Inputs go into temporary
  vectors (`eval/eval_node.rs:2707-2788`). Its code clears the ports at each
  tick, and a lazy node not ticked holds nothing (`eval/control_tests.rs:344-347`).
  So it never shows a stale value, but has nothing to show for an unticked
  node, and the wasm delta skips that node, so a viewer keeps its last value
  unmarked. The cache the bench adds keeps every output, as in the other
  builds. What the control writes reaches the graph a tick late, as in #405.
  Its pull request is closed.
- **The reference keeps 45% of its outputs,** in fields known only to its
  author. Its other values are locals of the tick, and it has no tree, so no
  statuses. The cheapest program is the hardest to inspect.

For an inspector, **#409 is the best placed, then #396.** They are the only
builds that keep each node's last call whole, and #409 shows a failed one;
what they lack is two private fields made readable. #257's port surface is
the most complete, but it keeps no inputs and lets unticked nodes vanish.
#404 keeps no values a node ran with, and #405 hides its placed nodes. The
reference keeps the least.

### Found, not applied

- **The glance timer as one function of time.** The designs still run its five
  nodes (since, phase, modulo and two comparisons) at every tick while someone
  is there and Pip is not talking: 0.4 calls a tick. Written as one
  `@pip/motion` window with a horizon, it would run when the window opens or
  closes, as the reference's does: 0.8 µs a tick.
- **A lipsync function with a horizon on the playhead.** The viseme changes
  only in its crossfades, 0.09 times a tick, but its node runs whenever the
  playhead moves, 0.19 times: 0.2 µs a tick.
- **Selections, holds and field reads run natively.** About 5 calls a tick in
  every design only select (`if`), hold or read a field, and pay 2 µs each in
  this model: 10 µs. Run in the executor without a module call, they would cost
  a node tick each. The flat 2 µs a call cannot tell a selection from a blend;
  work by kind of function is the model's next refinement.
- **#409's comparison inside its node's tick,** as its pull request suggests:
  at most its full-node cost, 0.7 µs a tick here.
- **#405's groups placed after the tree's body,** which removes its tick of
  lag; the cost stays the same.
- **Fusing a chain of arithmetic** (the eyes' subtract, scale and clamp) into
  one step. Bespoke code does it, and the counts do not credit it.

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
2. **Control timing in #257.** Only the motion's lag is modeled. Statechart
   transitions that cross ticks through latched state are not.
3. **A Rust replay of the run,** as for the Quori workloads, to replace the
   modeled costs with measured ones.
4. **Which of the bench's optimizations each design takes up.** Lazy
   conditionals, time horizons, latched arguments, native polls and wake on
   completion are the bench's proposals (§10). Some change a framework's
   evaluator; latched arguments and native polls also change Arora's async
   contract, with a leaf that reads its arguments once and a call that signals
   its completion. Each pull request's author to say which their design takes.
5. **Work by kind of function.** The flat 2 µs a call charges a selection what
   it charges a blend (§10, found, not applied). Measured costs per kind of
   function would weigh caching against recomputing for what each function
   actually costs.
