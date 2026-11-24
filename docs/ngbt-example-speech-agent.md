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
     resume, halt  ▼      │ playing, done                     │
                  SPEECH PIPELINE ──── viseme, playing ───────┘
```

- **The statechart decides.** It holds the states and their transitions, and
  is what a design's behavior tree has to implement (§3).
- **The motion graph transforms and blends, every tick.** It turns the states,
  the live input and the assets into the nine channels (§4).
- **The speech pipeline transforms text and speech, asynchronously.** One run
  per turn, started, held, resumed and halted by the statechart (§5).

What passes between them:

| From → to | What |
|---|---|
| inputs → statechart | voice activity (`vad`), recognized words (`asr`), face visible |
| inputs → motion graph | face position |
| statechart → motion graph | the states and when each started; the active emotions and their trigger times; the gesture |
| statechart → pipeline | start a run, hold, resume, halt; start a judge |
| pipeline → statechart | a sentence arrived (with its emotion and gesture), audio ready, playback started and ended, a tool call started, the reply ended, a verdict |
| pipeline → motion graph | the viseme the speech reports at its playhead, whether speech is playing |

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
| Judging | verdict `invalidated` | Thinking | The held run is halted. A run starts on the round and the new words |
| Judging | VAD on | Listening | The judge is halted |
| Thinking | sentence 1 arrives | Thinking (preparing) | Its emotion triggers; the thinking expression fades out |
| Thinking (preparing) | its audio starts | Speaking | |
| Speaking | the last sentence ends, follow-ups included | Idle | The turn is remembered |
| any | attention enters Nobody | Idle | The interaction resets: a run is halted, the round is cleared |

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
the face position, the viseme and the assets, and writes the
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
  out over 80 ms; `open` and `stretch` blend toward the viseme the speech
  reports, crossfaded over 50 ms from the tick its code changes, and `corners`
  adds the viseme's offset to the posture's. The mouth mixes lipsync and the
  emotion fades at all times.
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
                                                        └─ sentences {say, emotion, gesture?} ─▶ tts ─▶ play ─▶ viseme
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
| play | the audio's clock | the audio's length | audio → the viseme at its playhead; pausable |
| remember | async, fire and forget | | the turn → memory |

- **Tools:** each LLM round either calls a tool or starts the reply, with at
  most two tool rounds a run.
- **Fillers:** when a tool call starts in a run that is not held, a stored
  filler line plays through the lipsync path.
- **Held runs** may call info tools. Playback, filler lines, emotions and
  action tools wait until the run is handed back, so a halted run leaves no
  effects.
- **The judge** decides whether the reply can continue after the user adds
  words, and writes what to add if it can.
- **Remember** records the turn when the reply ends or is halted, partial
  replies included.
- **The mocks are deterministic.** The mock LLM's reply depends on which facts
  reached its prompt (§6), so a design that builds the prompt before the facts
  arrive says the wrong thing.

Long-running work follows Arora's call contract
([async-functions.md](async-functions.md)):

- **A call is polled.** A function returns `Running` and is called again at
  every step, its arguments encoded and sent again. A provider does the work
  off the tick, in a producer, and the call only checks on it.
- **A run is keyed by its arguments.** A call carries no run id, so a
  provider finds its run by what it was sent: a prompt that changed while the
  llm waits is another call.
- **Nothing tells a provider its leaf stopped.** A speech provider plays on
  until it has seen no call for 250 ms, then drops the run; called again with
  the same text, it starts the sentence over (vizij-arora-tts, say-piper). The
  bench's provider keeps synthesized audio by its text, so it starts over at
  once; vizij-arora-tts fetches the audio again.
- **Outputs come back with each call.** The speech provider reports the
  viseme at its playhead, sampled every 15 ms, as an output of `play`.

Every design runs on Arora and keeps this contract. The bespoke reference does
not have to; §10 shows what the contract changes.

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
            llm   → {happy} "Sunny all day, twenty-four degrees!"        halted: never heard
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
   - 10.6: `invalidated`. Run 1 is halted, its second LLM round in flight.
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
  wait over its 100 ticks. Under Arora's call contract that is 100 polls, each
  a call that sends the prompt again.
- **Each build at its best.** Each build is measured with every optimization
  its framework can take, each checked to leave its channels exact (§10).
- **Effects.** Per run: one rag and one recall; one prompt per LLM round; one
  tool call; one TTS per sentence. One judge per confirmed interruption. One
  filler line, from run 2.
- **Stale output.** No words, emotion, filler line or action tool from a held
  or halted run.
- **Barge-in.** Ticks from the VAD to the audio pausing.
- **Resume.** A resumed sentence continues from its paused playhead, or is
  counted as starting over.
- **Lipsync.** The mouth's lag behind the audio, in ms: from the tick the audio
  enters a viseme to the tick the motion reads it.
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
  moment. The caption marks a sentence paused, playing on after its leaf
  stopped, or stopped.
- **The call contract.** Every design replays the reference's run under
  Arora's call contract (§5) and is checked against it. Its scorecard says
  what the contract changes against the reference, whose ghost shows it.
- **The system.** Every part composed in one drawing, from what a tick reads
  to what it changes. Every connector comes from what a node says it reads and
  writes (§10), so none is drawn by hand. The columns, left to right:
  1. **What a tick reads:** the world's inputs, and under them the store as the
     tick starts, as the tick before left it.
  2. **The nodes the tree reads** (`@round`, `@said`, `@emotion`, `@turn`),
     each connected from the keys it is computed from.
  3. **The trees.** A check is connected to what it tests, and a call to its
     arguments.
  4. **The store as the tree leaves it,** each key connected from the nodes that
     write it. Under it are the providers, with a dotted connector from each leaf
     that calls one. That connector carries the arguments out at every poll and
     brings back Running, or the result the leaf writes to its keys. A provider
     shows what its calls sent and what it answered, and the speech provider
     shows the audio playing on after its leaf stops polling it.
  5. **The motion graph.**
  6. **The outputs,** each channel beside the node that writes it.

  How to read it:
  - **A key written and read in the same tick.** A node ticked after a key's
    writer reads the write, as the reply run reads the utterance `listen` wrote.
    That connector runs back from the right, and shows only at the ticks it
    happens.
  - **What the tick did.** Each part is coloured in the words of Arora's
    behavior trace (#448, `crates/arora-behavior/docs/trace.md`):
    - **Tree nodes and calls.** A tree node is lit when the tick reached it,
      and a leaf when it made its call. A detached node is blue when it was
      called, amber when it answered from its last call.
    - **Keys and inputs.** Blue when it changed; amber when a node that ran
      read it unchanged.
    - **Providers.** Lit when called.
    - **Connectors.** One between two blue parts is the change passing. One
      into or out of an amber part is a read. The trees' own connectors follow
      the running path.

    A leaf that has finished answers from its status: it reads nothing and
    calls nothing.
  - **A node settling on its own.** A damp, a slew or a hold reads its own
    state, which its last call wrote back. On main, that state is the call's
    `state` parameter, beside `dt: {arora/dt}`. It is drawn as the loop on the
    node's corner, blue while the last call moved it. A slew still closing on
    a target that no longer moves runs with nothing before it lit. So does
    `sp_w` at the first tick, going from no state to its first.
  - **A node's connectors.** Selecting a node picks out all of them.
  - **The order of a tick** is numbered per design. Where the motion reads the
    keys a tick late (#405, #257), those connectors are dashed.
  - **Providers stand apart.** A provider is an element of its own, not a
    detail of a node. It is a module with its own runs, keyed by what they
    were sent, and its audio outlives the leaf. Its inspector shows its calls
    and the bytes they sent at each tick, and a call leaf's inspector shows
    what its call sends.

  A tick's statuses are the tree's, the tick a leaf finishes included: its
  write changes the conversation's state, and the next tick reaches the branch
  it leads to. At tick 0 the store holds what the behavior loaded with, so
  nothing in it changes.

  Every lit part has a lit or read connector into it, except for five
  mismatches between the bench's simulation and its trees. The view shows them
  as they are:
  - **The judge's follow-ups.** They stream for 1 s after its verdict, and the
    cost model polls the judge for them. But the judge's leaf returned at the
    verdict, and no other leaf polls it.
  - **The simulation enters a state in the tick the leaf before it
    finishes.** The audio starts or resumes as `await`, the judge or `listen`
    finishes (16.60, 19.20 and 25.30 s), a tick before `play` first polls.
    Pip ponders from 10.60 s, a tick before `show_thinking` writes it.
  - **#404's executor ticks a leaf again only when its status changes,** not
    when what it reads changes. Every design runs its control tree on that
    executor. So `set_present`, which reads `{conversation/state}`, is not
    ticked when Pip starts and stops talking (5.00 and 27.51 s), though its
    write changes.
  - **`remember`** is called as the reply ends, but the reply run reaches it
    once the last sentence is synthesized, before it is spoken.
  - **A faded emotion** leaves `{emotion/active}`, and no node writes that.
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
  parent checked); none, not visited, fainter when not needed. A store key that
  changed while no node read it is amber too: its readers sit on a branch a
  conditional did not take, so the change was noted (its readers marked stale,
  or a version bumped) and its value not read. `{face/y}` is this on 1,381 of
  the 3,961 ticks it changes. The reference and the foil run no graph: a node
  is blue where their code computes it, on the branches it takes, when it or
  what it follows from changed. Edges between two blue nodes are blue, edges
  into or out of a visit amber. A damp, a slew or a hold has a loop on its
  corner: its state, blue while its last call moved it. A tree node's border is its status. A node can
  be dragged to untangle a graph, with a reset per part.
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
  reference, the foil and the designs: each one's average tick, what of it
  goes to the engine's path, and its work.
- **Inspecting values, at a glance.** What an inspector finds in each build
  over the run (§10, Inspecting values).
- **The assets.** Every asset of §6, with the live ones following the left
  side: the postures showing, the prompts built, the memory written.
- **The implementations.** The reference, the spec as bespoke code, which
  every design is checked against, under the call contract; a naive foil, the reference's code with
  three rules broken for the scorecard to catch (it halts the reply on any
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

Every tree node says what it reads and writes, and every view is drawn from
that: the stored forms, the outline, the tooltips, the inspector, the cost of
a poll, and the system view's connectors.

| Node | Reads | Writes |
|---|---|---|
| A check | its test: `{vad} or {conversation/state} == "listening"` | |
| A call | its arguments: `agent.rag({reply/utterance})` | its outputs, bound to keys: `{reply/docs} = …` |
| A set | its values; with `on entry`, its own key, to know its state is entered | its key, and with `on entry` the keys written only then: `{attention/since}`, an emotion triggered |
| FOR EACH | the list it walks: `{reply/answer}` | the key it puts each item in: `{reply/sentence}` |
| The task run | `{reply/run}`, and `{reply/held}`, which gates what it shows | |

Every key a node reads has a writer: the world's inputs, a tree node, or a
leaf writing what its provider answered. The bench checks this when it loads.
A provider learns what to do only from a call's arguments, so a call sends
every key it needs: `play` sends `{reply/audio}`, and `listen` sends `{vad}`
and `{asr/words}`.

None of the open pull requests says what an async node is, or how a paused
reply is held. Each design below therefore carries a proposal for those
parts. The proposals are the bench's, for each pull request's author to
confirm or replace.

The four tree designs (#396, #409, #404, #405) share these parts:

- **The reply run is a task run** the conversation spawns, with Arora's spawn
  and halt. It outlives the branch that started it. The tree writes
  `{reply/held}` to gate what it shows, and halts the run to discard it.
- **Its stages are module-call leaves,** returning `Running` until done and
  polled at every tick. This is Arora's call contract (§5), and the way
  arora-dataflow-nodes places external calls and task runs.
- **The answer is spoken as it streams.** FOR EACH runs beside the answer
  loop under a `PARALLEL`. It synthesizes each sentence as the llm brings it,
  and stays Running for the next while the llm streams.
- **The prompt is built once a call,** by a leaf under `SEQUENCE*`
  (`sequence_star`), which resumes the llm past it while it waits. `{.prompt}`
  keeps it, and the llm leaf sends it at every poll. Built by a node that runs
  when its arguments change, the prompt would change while the llm waits, three
  times in run 2's second call as the emotion moves, and each change would be
  another call to the provider.

| Design | Motion graph | When the motion sees the statechart | Prompt | Split into |
|---|---|---|---|---|
| #396 detached nodes (as #407 runs it) | 77 detached calls, pulled at their first read and cached by their arguments; sinks publish after the tree | The same tick | A leaf under `SEQUENCE*`, once a call | Subtrees for the conversation and attention, a task run for the reply, detached nodes grouped by part |
| #409 full tree nodes | The same, each call also a full tree node | The same tick | The same | #396's file |
| #404 incremental tree | One node-graph shape per group, `PARALLEL[ leaf …, RUNNING ]`, leaves on change and bound to keys, after the statechart | The same tick | The same | One shape per group under the root `PARALLEL` |
| #405 macros, placed | One macro per group, placed at the root in a `WITH` before the tree | A tick late | The same; a macro placed before the llm would build again whenever the emotion moved | One macro per group, authored and placed |
| #257 BT in NG | Vizij graphs: control nodes for the conversation, attention and reply run; the motion one graph whose nodes keep their outputs until a node they read changes and marks them | A tick late: a state the control writes reaches the motion through the store | An action under `sequence_with_memory`, once a call; its output port, kept, is what the llm action sends | One graph per region, joined to the motion by keys; the motion one graph, shown a group at a time, since a store hop between its groups would put the eyes a tick behind the head |

#257's pull request was closed on 21 October in favor of #396.

### Arora's call contract

Every design runs on Arora, so every one keeps its call contract (§5); the
bespoke reference does not have to. Each design therefore replays the
reference's run under that contract, and is checked against it. What the
contract changes is the same for every design, since it lives in the
providers, not in the trees:

- **The audio plays on 250 ms at each cut-in,** twice: nothing tells the speech
  provider its leaf stopped. Every design fails "Audio pauses as Sam speaks"
  for it, and for nothing else.
- **A held sentence starts over** when it is handed back, twice: the provider
  dropped its run after 250 ms without a call, and the same text is a new run.
  The reply ends 0.75 s after the reference's.
- **The mouth reads the viseme the provider reports,** a code sampled every
  15 ms, at most 10 ms after the audio for #396, #409 and #404, and 20 ms for
  #405 and #257, which read the store a tick late. The reference reads its own
  playhead.
- **Every tick of a wait is a module call** that sends its arguments again: a
  second on the llm is 100 calls, each carrying the prompt, about 650 bytes.
  In CPU that is little: 70 µs a second.

Three changes to the contract would cut the polling, and the bench used to
apply them as optimizations. They save little, 0.6 µs and 5 node ticks a tick
together, and none touches what the contract changes in the run:

| Change | What it does | Saves a tick |
|---|---|---|
| Latched arguments | A waiting leaf sends its arguments once, when its call starts; a run id names the call after that | 0.08 µs |
| Native polls | A waiting leaf checks its call's handle, `start → handle, poll → Option`, without a module call | 0.45 µs |
| Wake on completion | A provider signals its call done, so a waiting leaf stays clean and its branch is not ticked while it waits | 0.14 µs |

What would change the run is a halt the provider hears, which ends the
250 ms the audio plays on, and a pause it is told of, which keeps a held
sentence's place instead of starting it over.

### How the work is counted

The bench runs on one clock at 100 Hz, and nothing in it is async: a wait is
a leaf that answers `Running` until its work is done. Under Arora's call
contract a waiting leaf is polled at every tick: it reads what it sends, and
calls its module through the engine, so a second of waiting on the llm is 100
calls, each sending the prompt.

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
  - Every tick of a wait is a poll: a node tick, a module call, and a check
    for each store key or cell it reads to send its arguments. A node is a
    node tick that answers from its cache (detached: a check for each
    argument; versioned cells: one check) or runs again.
  - The motion graph follows each design's rule in the table above, with the
    optimizations below: in #396, #409 and #257 a node that recomputes to a
    new value marks its readers, and only marked nodes are visited; #404's and
    #405's cells are versioned, and only the leaves whose inputs changed are
    ticked.
- **Modeled µs.** The designs pay the NGBT spectrum's per-operation rates,
  plus 2 µs of work a call and 2 ms a prompt build. A poll that answers
  `Running` does no work and pays only its module call. The reference pays for
  its own work and nothing else.
- **The engine's path.** The spectrum's rates were measured with an
  in-process bridge, which hands a call its values. Through Arora's engine, a
  call's arguments are encoded into an Arora Buffer, decoded by the module and
  cloned for the function, and its result is encoded and decoded back, even for
  a module linked into the runtime. A wasm component adds the copies into and
  out of its memory, and does its half in wasm. The bench charges 0.3 µs a call
  and 0.4 ns a byte into a linked module (`@arora/dataflow`, and the providers
  `@pip/agent`, `@pip/speech`, `@pip/tools`, which need threads, the network
  and a speaker), and 1 µs and 1.2 ns a byte into a wasm component (the
  example's `@pip/motion`, `@pip/expression`, `@pip/lipsync` and
  `@arora/animation`), each call's bytes counted from its values. These two
  rates are modeled from the code path: no bench calls through the engine yet
  (§12).

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
| Time horizons | A clip, an envelope, the thinking glance or a viseme's crossfade says when its value next changes, and its node does not run until then | the bench | all |

Latched arguments, native polls and wake on completion, which this table used
to list, change Arora's call contract, so no build can take them (Arora's call
contract, above).

**Invalidation on #396 and #409 costs 0.24 µs more a tick, and is kept.** A
detached node is held by no node and has no list of its readers: pulled, it
checks its arguments to know its cached value still holds, and they are
detached nodes too, so every needed node is visited and compared at every
tick. With invalidation the executor builds each node's readers when the file
loads, and a node that recomputes to a new value marks them. That halves their
node ticks, from 70 to 37 a tick, and cuts their checks from 108 to 64; they
then visit what #257 visits, node for node. In µs it costs more, since a value
comparison here (9.5 ns) costs less than the version write that marks a reader
(38 ns), and 28 of the 75 needed nodes change at every tick. For #257 it saves
its input and output nodes' visits, and pays.

Two alternatives cost more. Versions instead of value comparison (#396's
proposal, §9), measured against invalidation, cost 0.7 µs more: nearly
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
  fade, the thinking glance or the glance away next changes, and that the
  viseme moves only in the 50 ms crossfade after its code changes.
- **Its own player.** It pauses the audio at a cut-in and resumes it where it
  paused, and reads the viseme at its own playhead: no call contract stands
  between it and its speech.

It costs 55 µs a tick and computes 26.6 of the graph's functions a tick.

At the graph's granularity, 28.2 functions a tick are needed and change value.
That is the floor for an exact implementation that computes every one of them,
and the designs run 28.8. The bespoke reference runs fewer, 26.6:
it takes a branch where the graph has a selection node, skips the arm's blend
while no wave plays, and knows a schedule a design does not (the glance away).

### What the bench shows over the whole run

- **#396, #409 and #404** produce the nine channels of the reference's run
  under the call contract at every tick.
- **#405 and #257** produce them a tick late wherever the statechart drives
  them. The mouth reads each viseme 10 ms later, 20 ms at most behind the
  audio, under the 45 ms people notice, and the eyes move a tick late when a
  face appears.
- **Work a tick,** on average, with one second waiting on the llm as node
  ticks · executions · checks:

  | Implementation | µs | Engine's path | Node ticks | Executions | Checks | 1 s on the llm | Prompt builds |
  |---|---|---|---|---|---|---|---|
  | Reference | **55** | — | 28 | 27 | 18 | 0 · 0 · 100 | 4 |
  | #396 detached nodes | 78 | 12.3 | 38 | 30 | 64 | 100 · 100 · 100, 70 µs | 4 |
  | #409 full tree nodes | 78.7 | 12.3 | 38 | 30 | 64 | the same | 4 |
  | #404 incremental tree | 78.4 | 12.3 | 51 | 30 | 69 | the same | 4 |
  | #405 macros, placed | 78.3 | 12.3 | 49 | 30 | 69 | the same | 4 |
  | #257 BT in NG | 78.4 | 12.3 | 46 | 30 | 64 | the same | 4 |

- **The designs end within 0.7 µs of each other.** What is left is
  the 30 functions that change a tick, at 2 µs each, the engine's path for
  each call, and each framework's bookkeeping. #396, #409 and #257, with
  invalidation, visit only what changed: the same nodes at nearly every tick,
  and the same 64 checks. #396 and #409 tick the fewest nodes (38 a tick);
  #257 also visits its graph's input and output nodes (46). #404 and #405 tick
  51 and 49, with 69 checks. The polls are the same in every design: one node
  tick and one call a tick on average.
- **The bespoke reference is the cheapest,** 22.8 µs a tick under the best
  design. It makes 3.2 fewer calls a tick (6.4 µs): the selections it takes as
  branches, the arm's blend it skips while no wave plays, the glance away's
  schedule, and the polls its event loop does not make. It pays no engine's
  path (12.3 µs), and the other 4 µs is bookkeeping a framework pays and it
  does not.
- **The prompt.** Every design builds it once per call, four times, as the
  reference does.
- **Every design fails one check, and only for the call contract:** the audio
  plays on 250 ms after Sam starts speaking. The reference passes every check.

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
| #396 detached nodes | 100% | 13% | 98% | 0% | reported by a trace (#448) | a trace: statuses, each detached node called, cached or idle, watched outputs; `detached_output`; `observe_detached` |
| #409 full tree nodes | 100% | 13% | 98% | 0% | kept; each call's `detached_status` | as #396, with a status and a trace line per call |
| #404 incremental tree | 100% | 13% | 86% | 0% | kept, private | nothing outside the tree |
| #405 macros, placed | 100% | 13% | 93% | 8.6% | kept, private | less than #404 for placed nodes |
| #257 BT in NG | 100% | 13% | 93% | 8.6% | the `out` port, typed `Status` | `eval_all`, `get_outputs_delta` |

- **Only main pushes a node's values, through a trace.** Each design keeps
  them in a cache, a cell or a port, for the next tick to read. The store is
  the only push channel (`subscribe` in `arora-types/src/data/store.rs:322-345`,
  and the bridge's `subscribe` and `values_changed`). On main it carries a
  trace (#448, `crates/arora-behavior/docs/trace.md`), a task run that writes
  a report to its feedback key at the end of each tick that changed
  something. A report holds each node's status, or not reached; each detached
  node's activity: called, cached, or idle when nothing read it; and the
  outputs of the nodes it watches, mutable parameters included. It holds no
  node's inputs. The branches of #404, #405, #407 and #409 predate it. A
  tree-local cell reaches no observer (`variable.rs:109-116`).
- **Every build holds the same stale values,** 13% of its node outputs: they
  take the same laziness, so the same nodes go unread. They differ in what they
  keep beside the output, and in whether anything outside the executor can
  tell a stale value from a current one. No build says a value is stale. Main's
  trace comes closest: a node reported idle was not brought up to date. #257's
  code sidesteps it by keeping nothing for a node it did not tick.
- **#396 and #409 keep the most.** A detached node's cache holds its last call:
  the outputs, and the arguments it ran with, which it compares and lends to
  the call (`detached.rs:188-201, 1008-1035, 1065-1077` on main). An input row
  shows what the node ran with at every tick after its first call, older than
  what feeds it now at 19% of input-ticks. `detached_output` reads the outputs,
  and a trace reports them for the nodes it watches, with each node's activity,
  without running anything. The arguments and the cache's tick are private: an
  accessor for each would give an inspector the whole call, and staleness by
  comparing the arguments with their sources. `observe_detached` keeps a node
  current by running it at every tick (`behavior_tree.rs:102-108`), which
  advances a stateful node on a branch not taken: watching that way changes
  what runs. #409 adds a status
  per call, `detached_status`, which is none before the first call and after
  a failure, so a failed call shows, and a trace line per real call
  (`behavior_tree.rs:98, 1295` on #409).
- **#404 keeps statuses and cells, but no values of a node's own.** A leaf's
  output is the cell it is bound to, and its inputs are the cells it reads, as
  they are now (`behavior_tree.rs:1398-1406`). Those are what it ran with only
  while the leaf is clean: an input row shows a newer value at 14% of
  input-ticks, and what the node ran with is gone. A cell counts versions only
  while it has readers. The dirty bit and the statuses are private
  (`incremental.rs:49-58`), and nothing reaches an observer outside the tree.
- **#405 shows less than #404.** A placed node is an `_on_change` leaf with a
  private port, which `observe_detached` and `detached_outputs` do not reach
  (`docs/proposal-placing-detached-nodes.md:264-270` on #405). Unplaced nodes
  keep #396's cache, so one file has two regimes. Its groups run before the
  tree, so at 8.6% of ticks the graph and the store hold different values of
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
  closes, as the reference's does: about 1 µs a tick.
- **Selections, holds and field reads run natively.** About 5 calls a tick in
  every design only select (`if`), hold or read a field, and pay 2 µs each in
  this model, and the engine's path: 11.5 µs. Run in the executor without a
  module call, they would cost a node tick each. The flat 2 µs a call cannot
  tell a selection from a blend; work by kind of function is the model's next
  refinement.
- **The call contract's changes** (Arora's call contract, above): latched
  arguments, native polls and wake on completion, 0.6 µs a tick together; a
  halt the provider hears and a pause it is told of, which change the run.
- **#409's comparison inside its node's tick,** as its pull request suggests:
  at most its full-node cost, 0.7 µs a tick here.
- **#405's groups placed after the tree's body,** which removes its tick of
  lag; the cost stays the same.
- **Fusing a chain of arithmetic** (the eyes' subtract, scale and clamp) into
  one step. Bespoke code does it, and the counts do not credit it.
- **Noting only the changes a needed node reads.** A store key's change is
  noted when no node needs it, its readers sitting on a branch a conditional
  did not take. The note marks them stale, or bumps a version. That happens
  0.35 times a tick in every design, nearly all of them `{face/y}`, while Pip
  wanders or holds a lost face's target. Leaving it for the branch to catch up when it is taken would save 0.017 µs a
  tick, not worth the bookkeeping.

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
   stages as module-call leaves, and the prompt as a leaf under `SEQUENCE*`.
2. **Control timing in #257.** Only the motion's lag is modeled. Statechart
   transitions that cross ticks through latched state are not.
3. **A Rust replay of the run,** as for the Quori workloads, to replace the
   modeled costs with measured ones.
4. **Which of the bench's optimizations each design takes up.** Lazy
   conditionals and time horizons are the bench's proposals (§10), and change
   a framework's evaluator. Each pull request's author to say which their
   design takes.
5. **Work by kind of function.** The flat 2 µs a call charges a selection what
   it charges a blend (§10, found, not applied). Measured costs per kind of
   function would weigh caching against recomputing for what each function
   actually costs.
6. **The call contract.** Whether Arora's contract gains a halt a provider
   hears, a pause, a run id or a completion signal (§10, Arora's call
   contract). The first two change what the robot does; the others only its
   polling.
7. **The engine's path, measured.** The bench models a call through the
   engine at 0.3 µs into a linked module and 1 µs into a wasm component, from
   the code path. The behavior-tree bench of #434 runs natively and in wasm;
   given a mode that calls through the engine, it would measure both.
