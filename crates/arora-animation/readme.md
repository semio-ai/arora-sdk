# arora-animation

Keyframe animation for Arora: the clip format, exact evaluation with
velocities, and players that run unattended on a device clock.

```rust
use arora_animation::{Clip, Engine, LoopMode, Timing};

let clip = Clip::from_json(r#"{"tracks":[{"id":"t","animatableId":"elbow","points":[
    {"id":"a","timeNs":0,"value":0},
    {"id":"b","timeNs":1000000000,"value":1.5,"transitions":{"in":"sine"}}]}]}"#)?;
let mut engine = Engine::new();
let anim = engine.load(clip, vec![]);
let player = engine.create_player("wave");
engine.add_instance(player, anim, 1.0, Timing::default());
engine.set_mode(player, LoopMode::PingPong);
engine.play(player);
let outputs = engine.step(16_000_000, 16_000_000); // device time and dt, in ns
# Ok::<(), arora_animation::ClipError>(())
```

The crate depends on no Arora runtime. The animation module
([`modules/animation`](../../modules/animation/readme.md)) declares it as an
Arora module, for devices and for Semio Studio's World; anything else
evaluates a clip with it directly.

## The clip

A clip is tracks, each animating one target, of keypoints at signed
nanoseconds of clip-local time: a keypoint may come before 0. A value is a
number (`f64`), a boolean or a string, one kind per track. Each side of a
keypoint carries the transition its author chose, which only a number track
reads:

| Transition | Document form | Plays |
|---|---|---|
| handle | `{"dtNs": …, "dv": …}` | a Bézier control point that far from its keypoint, in nanoseconds and value units |
| easing | `"linear"`, `"sine"`, `"quad"`, `"cubic"`, `"quart"`, `"quint"`, `"expo"`, `"circ"`, `"back"` | the easing's control point on its segment |
| inferred | `"explicit-handles"` | the tangent the neighbouring keypoints give |
| inferred, clamped | `"inferred-auto-clamped"` | that tangent, flat at an extremum and inside its segment's value range |

A side with none eases as `"cubic"`. A keypoint may also carry `pairing`
(`"paired"`, `"free"`), which an editor reads and playback does not.

The document form is JSON. A clip's `lengthNs` is its natural end: a player
given no window and no length plays it to there; absent, the clip ends at its
last keypoint. A stored Semio Studio animation of format 3 is the document
without `lengthNs`, whose editor fields (its id, groups, colours, viewport
extent) are ignored here; Studio sets the length when it delivers a clip.
`Clip::from_json` reads it and `Clip::to_json` writes it; clip format 1.x
(normalized stamps and handles) reads through `clip1::ClipV1::to_clip`.

## Evaluation

`Curve::new(track)` compiles a track; `sample(t)` evaluates it at a
clip-local time in nanoseconds, `sample_many` and `sample_numbers` over a
batch of times in one call. A sample is a value and, for a number, a velocity
in value units per second. `polyline` gives the points an editor draws a
number track with between two times: on the curve, evenly spaced in a
segment's Bézier parameter, so drawing a segment solves for time only at its
ends.

- **Exact.** A number segment is a cubic Bézier in (time, value); its time is
  solved for the curve's parameter to double precision, its value and
  velocity taken from the curve. Where the tangent is vertical in time the
  velocity is infinite.
- **The control points are Studio's.** `resolve` places them by the rules
  Semio Studio's editor draws a curve with, so a clip plays the curve its
  author saw. Resolution works in milliseconds, as the editor does: the
  inferred tangent normalizes a (time, value) direction, so the time unit is
  part of the rule.
- **Steps.** A boolean or string holds the value of the keypoint that starts
  its segment; at an interior keypoint's own instant the segment ending there
  still holds, just after it the keypoint plays. The last keypoint plays at
  its instant.
- **Holds.** Before the first keypoint the first value holds, after the last
  the last, with no velocity. Where keypoints share the first or the last
  time, the segments decide, as in the reference results: before the first
  segment the value it starts from, after the last the value it ends at;
  with every keypoint at one time, the first value before it and the last
  from it on.

## Playback

A `Player` is a playhead on a timeline, moved by the device clock (the step's
time, `arora/time` on a device). It measures how far it has travelled from
the device time it last started or changed speed at, in whole nanoseconds,
so two devices sharing a clock and a start stand at the same place at the
same time however each steps, and a player looping for days does not drift.

It plays within a window, by default its whole length; one outside its
window plays over its whole length until it enters the window. At a bound,
`once` stops (the player has ended), `loop` wraps, `bounce` turns back once
and then stops at the other bound, `ping_pong` turns back every time. These
are Studio's transport rules, with the travel that passes a bound carried on
past it. The speed is signed: negative plays backwards.

An `Engine` holds clips, players, and instances, each putting a clip on a
player at a weight and a `Timing`: clip-local time is
`(playhead − offset) × rate`, so a rate of 2 plays a clip, its handles with
it, twice as fast. A step writes one value per key and player. Within a clip
the first track on a target plays it; within a player, instances on one key
blend, numbers by weight and a boolean or string replacing what came before.
`bake` samples a clip at a fixed rate.

## Tests

```sh
cargo test -p arora-animation
```

- `tests/conformance.rs` holds every case of `fixtures/conformance/`,
  reference results Studio computes from its own curve logic, to within
  1e-9 of each track's range (values) and 1e-9 of the velocity's scale.
- `tests/fidelity.rs` plays 120 generated clips and a long, steep one through
  a player stepped at 100 Hz and a 100 Hz bake, and holds every frame to the
  curve computed independently.
- `tests/player_clock.rs`: an hour at 100 Hz on the timeline, a month of a
  looping player, a step equal to the bake at its time, devices stepping
  differently in lockstep.
