# arora-animation

Keyframe animation for Arora: animations of keypoints, evaluated exactly with
their velocities, and players that play them unattended on a device's clock.
An animation plays the curve its author saw in Semio Studio's editor, and two
devices sharing a clock play it in lockstep however each steps.

## Animations, players, outputs

An **animation** ([`Clip`](src/clip.rs)) is a list of tracks. A **track**
animates one **target**, a name its outputs carry (a store key, usually),
through **keypoints**: a value at a time. A value is a number, a boolean or a
string, one kind per track; a number moves along a curve between keypoints,
and a boolean or a string steps. Times are signed nanoseconds of the
animation's own time, its **local time**: a keypoint may come before 0.

A **player** ([`Player`](src/player.rs)) is a playhead on a timeline, moved by
the device's clock. The [`Engine`](src/engine.rs) holds animations, players,
and **instances**, each putting an animation on a player at a weight and a
[`Timing`](src/engine.rs). Each step moves every player to the device's time
and gives one **output** per target and player: the value there and, for a
number, its velocity.

```rust
use arora_animation::{Clip, Engine, LoopMode, Timing, Value};

// An animation: one track taking `elbow` from 0 to 1.5 in one second.
let animation = Clip::from_json(
    r#"{"tracks":[{"id":"t","animatableId":"elbow","points":[
        {"id":"a","timeNs":0,"value":0},
        {"id":"b","timeNs":1000000000,"value":1.5,"transitions":{"in":"sine"}}]}]}"#,
)?;

// A player playing it back and forth.
let mut engine = Engine::new();
let anim = engine.load(animation, vec![]);
let player = engine.create_player("wave");
engine.add_instance(player, anim, 1.0, Timing::default());
engine.set_mode(player, LoopMode::PingPong);
engine.play(player);

// Each step moves the players to the device time, in nanoseconds, and
// gives one output per target: here `elbow`, after 16 ms of play.
let outputs = engine.step(16_000_000, 16_000_000);
let (_revision, targets) = engine.output_keys();
assert_eq!(targets, ["elbow"]);
let sample = outputs[0].sample.as_ref().expect("a value");
assert!(matches!(sample.value, Value::Number(v) if v > 0.0 && v < 1.5));
# Ok::<(), arora_animation::ClipError>(())
```

The crate depends on no Arora runtime: a browser page or a simulator plays an
animation with it directly. On a device, the module `@arora/animation`
([`modules/animation`](../../modules/animation/readme.md)) gives a behavior
its players as functions; their outputs are values a behavior writes to keys.

## The animation document

An animation's document form is JSON, the core of a stored Semio Studio
animation of format 3; the editor's own fields (the animation's id, its
groups, a track's colour) are ignored. `Clip::from_json` reads and validates
it, and `Clip::to_json` writes it. A track's `animatableId` is its target.

Each side of a keypoint carries the transition its author chose, which only
a number track reads:

| Transition | Document form | Plays |
|---|---|---|
| handle | `{"dtNs": …, "dv": …}` | a Bézier control point that far from its keypoint, in nanoseconds and value units |
| easing | `"linear"`, `"sine"`, `"quad"`, `"cubic"`, `"quart"`, `"quint"`, `"expo"`, `"circ"`, `"back"` | the easing's control point on its segment |
| inferred | `"explicit-handles"` | the tangent the neighbouring keypoints give |
| inferred, clamped | `"inferred-auto-clamped"` | that tangent, flat at an extremum and inside its segment's value range |

A side with none eases as `"cubic"`. A keypoint may also carry `pairing`
(`"paired"`, `"free"`), which an editor reads and playback does not.
`lengthNs`, when present, is how long the animation plays when nothing else
bounds it; absent, it plays to its last keypoint.

Format 1.x (a duration in milliseconds, keypoints at stamps from 0 to 1 of
it, handles normalized to their segment) reads exactly into this form through
[`clip1::ClipV1::to_clip`](src/clip1.rs).

## Evaluation

`Curve::new(track)` compiles a track ([`curve`](src/curve.rs)). `sample(t)`
evaluates it at a local time in nanoseconds; `sample_many` and
`sample_numbers` evaluate a batch of times in one call. A
[`Sample`](src/value.rs) is a value and, for a number, a velocity in value
units per second. `polyline` gives the points an editor draws a number track
with between two times: on the curve, evenly spaced in a segment's Bézier
parameter, so drawing a segment solves for time only at its ends.

- **Exact.** A number segment is a cubic Bézier in (time, value). Its time
  is solved for the curve's parameter to double precision, and its value and
  velocity taken from the curve. Where the tangent is vertical in time the
  velocity is infinite.
- **Studio's control points.** [`resolve`](src/resolve.rs)
  (`resolve_segment`) places a segment's control points by the rules Semio
  Studio's editor draws a curve with. It works in milliseconds, as the editor
  does: the inferred tangent normalizes a (time, value) direction, so the
  time unit is part of the rule.
- **Steps.** A boolean or a string holds the value of the keypoint that
  starts its segment. At an interior keypoint's own instant the segment
  ending there still holds; just after it, the keypoint plays. The last
  keypoint plays at its instant.
- **Holds.** Before the first keypoint the first value holds, after the last
  the last, with no velocity. Where keypoints share the first or the last
  time, the segments decide: before the first segment the value it starts
  from, after the last the value it ends at; with every keypoint at one
  time, the first value before it and the last from it on.

## Playback

A player measures how far it has travelled from the device time it last
started or changed speed at, in whole nanoseconds, computed exactly rather
than summed step by step. Two devices sharing a clock and a start stand at
the same place at the same time however each steps, and a player looping for
days does not drift.

A player plays within a window, by default its whole length; one outside its
window plays over its whole length until it enters the window. At a bound,
the [`LoopMode`](src/player.rs) decides: `Once` stops (the player has ended),
`Loop` wraps, `Bounce` turns back once and then stops at the other bound,
`PingPong` turns back every time. These are Studio's transport rules, with
the travel that passes a bound carried on past it. The speed is signed: a
negative speed plays backwards.

An instance maps its player's playhead to the animation's local time as
`(playhead − offset) × rate`, so a rate of 2 plays an animation, its handles
with it, twice as fast. A step writes one value per target and player.
Within an animation the first track on a target plays it; within a player,
the instances writing one target blend, numbers by weight and a boolean or
string replacing what came before. `Engine::output_keys` gives the target of
each output position, per player in creation order, and a revision that
changes only with a structural edit (an animation reloaded or unloaded, a
player or an instance added or removed). A command takes effect at the
device time of the last step; before the first step, at its start
(`now − dt`).

`Engine::bake` samples an animation at a fixed rate over a window of its
local time.

## Reference

| Limit | Value |
|---|---|
| Fastest speed, either way ([`MAX_SPEED`](src/player.rs)) | 2^20 times device time; the engine refuses a faster one |
| Most samples one bake takes, over all its tracks ([`MAX_BAKE_SAMPLES`](src/engine.rs)) | 2^20 |
| Time | `i64` nanoseconds ([`Nanos`](src/time.rs)); a local time is an `f64` of nanoseconds, keeping a rate's fraction |

```sh
cargo test -p arora-animation
```

- `tests/conformance.rs` evaluates every case of
  [`fixtures/conformance/`](fixtures/conformance/README.md), reference results
  Studio computes from its own curve logic, to within 1e-9 of each track's
  range (values) and 1e-9 of the velocity's scale.
- `tests/fidelity.rs` plays 120 generated animations and a long, steep one
  through a player stepped at 100 Hz and a 100 Hz bake, and holds every frame
  to the curve computed independently.
- `tests/player_clock.rs`: an hour at 100 Hz on the timeline, a month of a
  looping player, a step equal to the bake at its time, devices stepping
  differently in lockstep.
- `tests/transport.rs`: Studio's transport rules, as a player carries them
  out.
