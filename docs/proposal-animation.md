# Proposal: animation in arora-sdk

Status: draft, for review.
Date: 2026-10-18.

Semio Studio's animations play in three places: in its editor, on the
devices its World emulates, and on real robots that run Arora on their own.
This document places animation in arora-sdk, so that all three run one
implementation: a library that evaluates a clip exactly, and a module through
which any Arora device plays clips, on its own clock, for as long as it runs.

In short:

- **Two crates.** `arora-animation` is the library: the clip format,
  exact evaluation with velocities, players, and the engine that steps them.
  It depends on no Arora runtime. `arora-animation-module` declares it as an
  Arora module, host-linked or as a `wasm32-wasip1` guest.
- **They move from vizij-rs with their history**, as the node graph does
  ([behavior languages](proposal-behavior-languages.md) §4).
  animation-player's history comes along, since vizij-rs forked from it.
- **The module keeps its ids**: module, functions, parameters and records.
  A caller of vizij's module reaches this one unchanged.
- **Clip format 2.0** carries what an author made: keypoints at signed
  nanoseconds of clip-local time, and on each side of a keypoint an explicit
  handle in time and value units, an easing, or an inferred tangent. Clip
  1.x stays readable.
- **One curve.** A clip plays the curve Studio's editor draws: the library
  places each segment's control points by the editor's rules and evaluates
  the Bézier exactly. Numbers are `f64`; strings and booleans play as steps;
  the last keypoint plays at its instant; keypoints before 0 play.
- **Players run on the device clock**, in integer nanoseconds measured from
  their last start, so they do not drift; the engine owns looping and
  bouncing, with Studio's transport rules.
- **Screen space stays in the editor.** Arora takes clips in their own local
  time and values; Studio turns a gesture into an edit of a clip.

---

## 1. What exists

**In vizij-rs**, two crates hold animation, neither published to crates.io:

- `vizij-animation-core` 5.0.0: the clip model, sampling, players, blending
  and baking.
- `vizij-animation-module` (module version 2.0.0): the core declared as an
  Arora module with `#[arora_module::module]`. Its guest ships on npm as
  `@vizij/animation-module`.

The [behavior languages](proposal-behavior-languages.md) proposal moves
vizij's node graph into arora-sdk and keeps `vizij-animation-core` in
vizij-rs.

**In Semio Studio**, on the Studio-on-Arora branch:

- The World loads the module's npm guest into every emulated device, with
  the module's record ids copied by hand.
- A conversion (`toModuleClip`) turns each Studio animation into the
  module's clip.
- The editor draws curves from a 200 Hz bake.
- Studio's transport emulates looping and bouncing, because the module's
  player wraps each clip where Studio holds it.

**What 1.x cannot carry.** Measured against Studio's own curves:

- Stamps are `f32` fractions of the clip and the player's clock is `f32`
  seconds summed step by step. A 20 ms ramp at the end of a 10-minute clip
  is off by 0.2 % of its range. A 100 Hz clock is 274 ms off after ten
  minutes, and stalls after about 73 hours.
- Values are `f32`: a move from 100000 to 100001 is off by 0.4–0.8 %.
- Local time is clamped at 0, so a keypoint before an instance's start
  does not play.
- Handles are normalized to their segment, so an easing preset, an
  inferred tangent and handle pairing have no field. A handle on a flat
  segment cannot be stored.

## 2. Where it lives

| Path | Crate | What it is |
|---|---|---|
| `crates/arora-animation` | `arora-animation` 1.0.0 | The library: `clip` (format 2.0 and its JSON document form), `clip1` (format 1.x read into 2.0), `resolve` (control points), `curve` (evaluation), `player`, `engine`. Depends on `serde` alone, so it builds anywhere, `wasm32` included. |
| `modules/animation` | `arora-animation-module` 1.0.0 | The module: its records and functions over the engine, declared once as a contract for hosts and once as a module for the guest's entry points. `crate-type = ["cdylib", "rlib"]`. |

This is the layout `examples/policy-modules` sets: a library a module
needs, and a module crate built both host-linked and as a guest. The
library carries no Arora type, so anything that evaluates a clip uses it
directly: Studio's editor, through the page's wasm, and a simulator or a
`motioncore` node.

**The move.** The history moves with `git filter-repo`, by the procedure of
§4.2 of the behavior-languages proposal:

```sh
git clone https://github.com/vizij-ai/vizij-rs.git vizij-extract
cd vizij-extract
git filter-repo \
  --path crates/animation/vizij-animation-core/ \
  --path crates/interop/vizij-animation-module/ \
  --path animation-player/ \
  --path-rename crates/animation/vizij-animation-core/:crates/arora-animation/ \
  --path-rename crates/interop/vizij-animation-module/:modules/animation/ \
  --path-rename animation-player/:crates/arora-animation/animation-player/
```

vizij-rs began as a copy of `semio-ai/animation-player` and deleted its
`animation-player/` tree before it started `vizij-animation-core`. Taking
that path brings animation-player's commits under
`crates/arora-animation/animation-player/`, which the tip no longer holds:
`git log crates/arora-animation` reaches back to animation-player's first
commit. The merge commit changes nothing it imports; the commits after it
rewrite the crates onto arora-sdk. The pull request lands as a merge commit,
so the imported history stays.

The behavior-languages table gains the move (§4.1): `vizij-animation-core`
and `vizij-animation-module` move to arora-sdk as these two crates.

## 3. Semantics

### 3.1 Time

- **Clip-local time is signed integer nanoseconds** (`i64`). A keypoint may
  come before the clip's 0, and an instance whose offset puts it there plays
  it.
- **Evaluation takes an `f64` of nanoseconds**, so a time mapped through an
  instance's speed multiplier keeps its fraction of a nanosecond. A
  segment's time is taken relative to its start before it is converted, so
  precision holds at any distance from 0.
- **A player's time is the device's clock**, `arora/time`, integer
  nanoseconds (§3.5).

### 3.2 Values

A track's values are of one kind: numbers (`f64`), booleans or strings. A
number interpolates along the track's curve; a boolean or a string steps.

A clip 1.x track whose numbers are all `f32` keeps writing `f32`, so a
consumer that keyed a typed input by it is not broken. Every other number is
`f64` from keypoint to output.

Composite values (vectors, quaternions, colours, transforms) are not part
of format 2.0: Studio splits a composite into one number track per axis. A
1.x clip carrying one is refused. Composites join when the value vocabulary
moves into arora-sdk as `arora-shape`.

### 3.3 The curve

- **A number segment is a cubic Bézier in (time, value)**, from one
  keypoint to the next. Its two control points come from the start
  keypoint's leaving side and the end keypoint's arriving side.
- **The control points are placed by Studio's editor rules**
  (`resolveSegmentControlPoints` in `@semio/animation`), so a clip plays the
  curve its author saw:
  - an explicit handle is its own delta;
  - an easing places its control point on the segment by the easing's
    parameters;
  - an inferred tangent bisects the directions to the neighbouring
    keypoints; the clamped one is flat at an extremum and stays within its
    segment's value range;
  - every handle is then scaled along its own ray, never bent, so it stays
    within its segment in time and the segment's time stays monotonic.
- **Resolution works in milliseconds.** The inferred tangent normalizes a
  (time, value) direction, and the clamps compare against tolerances in the
  same units. The time unit is part of the rule: resolved in nanoseconds,
  the same clip would play another curve.
- **Evaluation is exact.** A segment's time is solved for the curve's
  parameter to double precision, and the value is the Bézier's. Nothing is
  baked into a polyline.
- **Velocities.** A number sample carries its velocity: the curve's slope,
  in value units per second. At a collapsed handle, where the time
  derivative vanishes, it is the limit of the slope; where the curve's
  tangent is vertical in time it is infinite, so a client checking a
  velocity limit sees it exceeded. Held values have no velocity.
- **The step rule.** A boolean or a string holds the value of the keypoint
  that starts its segment. At an interior keypoint's own instant the segment
  ending there still holds; just after it, the keypoint plays.
- **The last keypoint plays at its instant**, of every kind. A clip that
  plays once to its end rests on its last keypoint.
- **Holds.** Before the first keypoint the first value holds, after the
  last the last; of keypoints sharing the first or the last time, the last
  of them plays from that time on.
- **Order.** Keypoints play in time order; keypoints sharing a time, in the
  order the track gives them. A segment of no duration plays nothing of its
  own: the later keypoint takes over.
- **Selection.** Within a clip, the first track on a target plays it. A
  later track on the same target does not play, and a track with no
  keypoint claims its target and writes nothing. A client that wants a
  track not to play leaves it out of the clip it delivers: Studio leaves out
  muted tracks, and the next track on the target plays, as in its editor.

### 3.4 Instances and speed

An instance puts a clip on a player at a weight and a timing:
`local = (playhead − offset) × rate`.

- **`rate` is a speed multiplier**, Studio's timescale: 2 plays the clip in
  half the time.
- **Handles scale with it exactly as keypoints do.** The whole curve maps
  through the same affine time transform, so a clip's shape is the same at
  every speed. A velocity is multiplied by `rate`, into value units per
  second of the player's timeline.
- vizij's `set_time_scale` is a duration multiplier (`local = (playhead −
  offset) / time_scale`). It stays, as the inverse of `rate`; `set_rate`
  takes Studio's convention.

### 3.5 Players

**The clock.** A player measures how far it has travelled from the device
time it last started or changed speed at: `round((now − anchor) ×
|speed|)` whole nanoseconds, the product computed exactly in integers.
Nothing is summed step by step. So a player stepped at 60 Hz and one
stepped at 1 kHz stand at the same place at the same device time, and a
player looping an idle animation for months, or at any speed, is where its
period puts it.

**Starting in step.** `play_at(t)` starts a player at device time `t`:
until then it holds, and from then on it stands where it would had it
started at `t`, however its steps fall around it. Devices sharing a clock
and given one `play_at` play in lockstep.

**The bounds.** A player plays within its window, by default its whole
length (the latest end of its instances). A player outside its window plays
over its whole length, `[0, length]`, until it enters the window; from then
on the window holds it. Setting a window leaves the playhead where it is,
and a playhead outside the new window plays toward it as from a start
outside. At a bound, the loop mode decides:

| Mode | At a bound |
|---|---|
| `once` | stops on it: the player has ended |
| `loop` | wraps to the other bound |
| `bounce` | turns back the first time, then stops at the other bound |
| `ping_pong` | turns back every time |

These are Studio's transport rules. Studio sets its playhead on a bound and
drops the rest of the frame; a player carries the rest of its travel on past
the wrap or the turn, which is what keeps its position independent of how
the device steps.

**Direction.** The speed is signed; a negative speed plays backwards. A turn
reverses the heading without changing the speed. A speed of the other sign,
a `play` or a `seek` starts the rules afresh: no turn taken, the window not
yet entered, not ended. `set_reversed` gives a player the heading another
has after a turn, so one started late (an instance delivered mid-bounce)
plays alongside the others.

**Commands** take effect at the device time of the last step, so a client
reading the players after a command sees it. Before the first step they wait
for it and take effect at its start.

### 3.6 Outputs

A step writes one value per key and player. Within a player, the instances
writing one key blend: numbers and their velocities by weight; a boolean or
a string replaces what came before it in the step. The output table — per
player in creation order, each key in the order its instances first write
it — changes only with a structural edit, which gives it a new revision; a
consumer reads the keys again only then.

### 3.7 Screen space stays in the editor

Arora takes, stores and evaluates clips in their own local time and values;
an instance's timing is its only time mapping. Turning a pointer gesture
into an edit is the editor's: from the screen to timeline time, from
timeline time to the clip's local time (`Δlocal = Δtimeline × rate`), and
the snap that pairs two handles, an angle the person judges on screen. A
handle dragged at a speed other than 1 is stored in clip time, so it plays
under the cursor at that speed and keeps its shape at every other.

## 4. The API in both settings

### 4.1 Arora on its own: a robot

A device carries the module, host-linked into its runner or as a module
directory (a `header.json` beside the guest's `.wasm`). It does not receive
trajectories. A client delivers it clips and commands, and the device plays
them on its own:

- **Content.** `load_clip(clip)` (or `load_animation` for 1.x),
  `create_player(name)`, `add_instance(player, anim)`; and
  `remove_instance`, `remove_player`, `unload_animation`.
- **Transport.** `play`, `play_at`, `pause`, `stop`, `seek`, `set_speed`,
  `set_loop` (`once`, `loop`, `bounce`, `ping_pong`), `set_window`,
  `set_length`, and `set_reversed` (the heading another player has after a
  turn, for a player started alongside it); per instance `set_weight`,
  `set_start_offset`, `set_rate` (and `set_time_scale`).
- **The step.** A behavior ticks the module with `arora/dt` and
  `arora/time`, `step_values(dt_ns, time_ns)` returning the values by
  position in the table `output_keys()` returns (`step_with_velocities`
  adds each velocity), and writes them onto the device's inputs. Studio's
  stepping run is that behavior.
- **Feedback.** `player_states()`: each player's state, playhead, length,
  speed, loop mode, window, whether it ended or turned, and its instances.
- **Baking.** `bake` and `bake_with_derivatives` sample a clip at a fixed
  rate.

Nothing in the module reads or writes the store, and nothing depends on
the steps' cadence: a robot looping an idle animation for days stays on its
timeline, and a robot and Studio given the same clip, commands and start
play the same values at the same device time.

### 4.2 Arora in Studio

- **The World** registers the module host-linked in every device it
  emulates, one module state per device:
  `HostModule::from_exports(MODULE, arora_animation_module::exports(AnimationModule::new()))`.
  The page fetches no guest, and the World binds calls by name over the
  module's declared header.
- **Real devices** receive the same calls over the Studio Bridge: a clip
  delivered, a withdrawal, a transport command. Studio's World queues them
  as it does today.
- **The editor** evaluates through the library, in the page's wasm: a
  track and a batch of times in one call, values and velocities back, for
  the playhead's readouts, a value it inserts and the checks it marks
  violations with; the curves it draws come from `Curve::polyline`, points
  on the curve spaced by a segment's parameter. A batch crosses into wasm
  once per track, not once per sample.
- **The transport** sends commands; the engine owns looping and bouncing,
  and the timeline's playhead follows the engine's player.

### 4.3 The library

```rust
let clip = Clip::from_json(document)?;           // format 2.0, its document form
let curve = Curve::new(&clip.tracks[0]);
let sample = curve.sample(t_ns);                  // value, and velocity per second
curve.sample_numbers(&times, &mut values, &mut velocities); // a batch

let mut engine = Engine::new();
let anim = engine.load(clip, vec![]);
let player = engine.create_player("idle");
engine.add_instance(player, anim, 1.0, Timing { offset: 0, rate: 1.0 });
engine.set_mode(player, LoopMode::Loop);
engine.play(player);
let outputs = engine.step(now_ns, dt_ns);          // one value per output position
```

## 5. Clip format 2.0

### 5.1 What it carries

| Level | Field | Meaning |
|---|---|---|
| clip | `name` | |
| clip | `length` | How long it plays when nothing else bounds it, from 0, in nanoseconds; absent, to its last keypoint. |
| track | `id`, `name` | |
| track | `target` | What it animates: the key a player's output for it carries. |
| keypoint | `id` | |
| keypoint | `time` | Clip-local, signed nanoseconds. |
| keypoint | `value` | A number, a boolean or a string. |
| keypoint | `in`, `out` | The transition of each side: a handle `(dt, dv)` in nanoseconds and value units, relative to the keypoint; an easing (`linear`, `sine`, `quad`, `cubic`, `quart`, `quint`, `expo`, `circ`, `back`); or an inferred tangent, plain or clamped. Absent: `cubic`. |
| keypoint | `pairing` | `paired` or `free`: an editing constraint the clip carries for its editor. Playback ignores it. |

The format states the step rule (§3.3) and the default easing, so two
readers of one clip play one curve.

**What stays the editor's.** A clip carries what plays and what an author
must get back unchanged: the transitions as chosen, not the control points
resolved from them, so moving a keypoint re-resolves its neighbours'
inferred tangents. The rest of an animation document is Studio's and
travels beside the clip: the document's id, its track groups, a track's
colour and group name, the viewport's extent.

### 5.2 Its two forms

- **The document form** is JSON, the core of a stored Studio animation of
  format 3: `{ name, lengthNs, tracks: [{ id, name, animatableId, points: [{
  id, timeNs, value, transitions: { in, out, pairing } }] }] }`, a handle
  `{ dtNs, dv }` and a named transition its name (`"cubic"`,
  `"explicit-handles"`, `"inferred-auto-clamped"`: Studio's spellings).
  Fields it does not name are ignored, so a stored animation is a clip.
- **The record form** is what a module call carries: `Clip`, `ClipTrack`,
  `ClipKeypoint` and `ClipTransition`, records `…0200` to `…0203` of the
  module's id space, at version 2.0.0, taken by `load_clip` and
  `reload_clip`.

### 5.3 Format 1.x

1.x records keep their ids and `load_animation` reads them. A 1.x clip
converts exactly: a stamp becomes nanoseconds of the duration, a normalized
handle `(x, y)` becomes the explicit handle `(x × Δt, y × Δv)` on its
segment (the defaults `(0.42, 0)` and `(0.58, 1)` where a side has none),
and a cubic Bézier is unchanged by that affine map.

## 6. Versions and compatibility

| Crate or surface | Version | Change |
|---|---|---|
| `arora-animation` | 1.0.0 | New. |
| `arora-animation-module` | 1.0.0 (module 3.0.0) | New crate; the module's interface continues vizij's 2.0.0. Same ids. Adds `load_clip`, `reload_clip`, `set_rate`, `step_with_velocities`, `set_length`, `set_reversed`, the 2.0 records and `StepSamples`; `PlayerState` 1.3.0 adds `reversed` and `waiting`, `InstanceState` 1.2.0 adds `rate`; `set_loop` takes `bounce`. Behaviour changes (§3): the clock, `f64` numbers, the last keypoint at its instant, keypoints before 0, a window that no longer moves the playhead, commands taking effect at once. |
| `vizij-animation-core`, `vizij-animation-module` | — | No further release once vizij-rs depends on the arora crates. |
| `@vizij/animation-module` (npm) | — | Deprecated, or a wrapper over the new guest. |
| Semio Studio's stored animation | format 3 | Clip 2.0 plus the editor's fields. Studio reads 1, 2 and 3, writes 3, and writes 3 only once every client refuses a format newer than it reads. Published releases keep format 2. |

The first versions of the two crates are published by hand, then each
crate's trusted publisher is registered, and the release workflow publishes
every later version (behavior languages §4.2, step 5). The release
workflow's list carries both.

**The guest needs a release channel.** arora-sdk's release workflow
publishes crates only. A device that loads the module from a directory needs
the `wasm32-wasip1` artifact and its header from somewhere: a GitHub release
asset of arora-sdk is the simplest.

## 7. Decisions for review

1. **Animation moves with the node graph.** This reverses §4.1 of the
   behavior-languages proposal, which keeps `vizij-animation-core` in
   vizij-rs. *Recommended*: move it, as this proposal does.
2. **The module's ids stay.** *Recommended*: keep every module, function,
   parameter and record id, so nothing that calls the module needs a change.
   The alternative is ids in an Arora namespace, changing every caller once.
3. **Host-linked in the World, a guest for module directories.**
   *Recommended*: Studio's World host-links the module, which removes the
   page's fetch of the npm guest and the record-typed exports a
   header-loaded guest has to describe; the guest build stays for devices
   that load modules from a directory, released as in §6.
4. **The engine owns looping and bouncing.** This reverses the
   Studio-on-Arora plan's D7, which kept them in Studio's transport because
   vizij's player wraps each clip within its window. A robot playing on its
   own needs them in the engine. *Recommended*: the engine, with Studio's
   rules (§3.5).
5. **Clip 2.0's fields, and what the engine resolves.** *Recommended*: the
   clip carries the transitions as their author chose them (§5.1), and the
   engine resolves them by Studio's rules. Every reader of a stored
   animation then plays the same curve with nothing resolved for it in
   advance, and nothing stored can go stale. The editor keeps its own
   resolver for drawing handles; both are held to the reference set Studio
   generates (`crates/arora-animation/fixtures/conformance`). The
   alternative carries resolved control points only, resolved by whichever
   client writes the clip.
6. **vizij-rs's open clock fixes.** vizij-rs#193 keeps a player's time in
   compensated `f64` seconds, and vizij-rs#192 steps the module on the
   device's `arora/time`. *Recommended*: carry what they do here rather
   than land them in a crate that is moving. This proposal goes further on
   the clock: integer nanoseconds from the last start (§3.5), so no sum is
   kept at all.
7. **A bound carries the travel past it.** Studio drops it, frame by frame,
   which makes a looping player's position depend on its frame rate.
   *Recommended*: carry it, so devices stepping differently stay in step.

## 8. Alternatives

| Alternative | Why not |
|---|---|
| Keep animation in vizij-rs and fix it there | A robot running stock Arora could not play an animation without a Vizij crate, and Studio would keep converting its animations into a format that cannot hold them. |
| Revive `semio-ai/animation-player` | It has the integer clock, but not the Arora module, the records, the stepping surface or Studio's curve; vizij's line has thirteen more months of that work. |
| Clip 2.0 as resolved control points only | Studio would resolve before every delivery and store the result beside what the author chose; a moved keypoint would leave stored handles stale until the editor rewrote them. |
| Keep the 200 Hz bake for the editor | It is off by up to 30 % on a short ramp in a 10-minute clip, and its error grows with the clip's length. |
| A player clock summed with compensation | Exact to a rounding over hours, but still a sum: its position depends on the steps that built it. Measuring from the last start removes the sum. |
| Copy the code without its history | Loses `blame` and the reasons the commit messages carry. |

## 9. Later

- vizij-rs plays faces through `arora-animation-module` and drops its own
  copy, on vizij's schedule.
- Composite values, once `arora-shape` moves into arora-sdk.
- animation-player is archived, its history now here.
