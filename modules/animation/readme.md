# arora-animation-module

Keyframe animation, players and baking as an Arora module over
[`arora-animation`](../../crates/arora-animation). The package is version
1.0.0; its declared module is `arora-animation` version 3.0.0, id
`76697a69-6a00-0000-0d00-000000000000`.

## Host and guest

A host registers one state per device. Every function belongs to the
`AnimationContract` and is described for introspection; arguments match by
parameter id, independent of their order.

```rust
use arora_animation_module::{animation, exports, AnimationModule};
use arora_engine::module::HostModule;

let module = HostModule::from_exports(
    animation::ids::MODULE,
    exports(AnimationModule::new()),
);
// engine.register_module(module.id(), Box::new(module));
```

The `cdylib` is a `wasm32-wasip1` guest. Load its wasm bytes with
`animation::header(executor)`, naming the `wasm` executor. Each loaded guest
has its own state in its wasm instance. The guest entry points delegate to
the same contract methods as a host registration. Hosts should register
`exports(state)` so devices in one process each own their animation state.
`animation::ids` exposes the module, function and parameter ids.

## Records

Records derive their schemas and value conversions in Rust. Dynamic fields
carry Arora `Value`s on the self-describing value wire.

| Record | Version | Contents |
|---|---|---|
| `AnimationClip` | 1.1.0 | Name, duration in milliseconds, 1.x tracks |
| `AnimTrack` | 1.1.0 | Id, name, `animatable_id`, keypoints |
| `Keypoint` | 1.1.0 | Id, normalized stamp, scalar value, normalized transition handles |
| `TransitionHandle` | 1.0.0 | Normalized x and y |
| `Clip` | 2.0.0 | Name, optional signed `length_ns`, 2.0 tracks |
| `ClipTrack` | 2.0.0 | Id, name, target output key, keypoints |
| `ClipKeypoint` | 2.0.0 | Id, signed `time_ns`, scalar value, arrive/leave transitions, pairing |
| `ClipTransition` | 2.0.0 | Kind, `dt_ns`, `dv` |
| `TrackOutput` | 1.0.0 | Track id, default authored key, sampled value |
| `StepValues` | 1.0.0 | Output revision and values by position |
| `StepSamples` | 1.0.0 | Output revision, values and velocities by position |
| `OutputKeys` | 1.0.0 | Output revision and keys by position |
| `PlayerState` | 1.3.0 | Player id, state, position, length, speed, name, instances, loop mode, ended, window, reversed |
| `InstanceState` | 1.2.0 | Instance id, animation id, weight, signed start offset, time scale, rate |
| `BakedAnimation` | 1.0.0 | Frame rate, window in nanoseconds, tracks |
| `BakedTrack` | 1.0.0 | Target key, sample values and optional derivatives |

Both clip formats accept numbers, booleans and strings. Composite values
are refused. A 1.x track whose numbers are all `F32` outputs `F32`; every
other numeric track outputs `F64`. Integers read as double precision numbers.
Format 1.x converts through `clip1::ClipV1::to_clip`, preserving its timing
handles and curve.

Format 2.0 converts both ways with `arora_animation::Clip`. An arrive or
leave side has zero or one transition. `kind` is `handle`, an easing name
(`linear`, `sine`, `quad`, `cubic`, `quart`, `quint`, `expo`, `circ`, `back`),
`explicit-handles`, or `inferred-auto-clamped`. Handles use relative
nanoseconds and value units; other kinds write zero for `dt_ns` and `dv`.
Pairing is empty, `paired`, or `free`. Unknown names and non-scalar values
refuse conversion. Loading also validates uniform track kinds and finite
numbers and handles.

## Exports

- Loading: `load_animation`, `reload_animation` for 1.x; `load_clip`,
  `reload_clip` for 2.0. Create players with `create_player`, then attach
  clips with `add_instance` or `add_instance_with_weight`. Remove them with
  `remove_instance`, `remove_player`, `unload_animation`.
- Stepping: `step(dt_ns, time_ns?)` returns valued positions as
  `TrackOutput`s, each with the track id and default authored key.
  `step_values` returns values alone; `step_with_velocities` also returns
  velocities. `output_keys` gives their position table and revision.
- Transport: `play`, `play_at`, `pause`, `stop`, `seek`, `set_speed`,
  `set_loop`, `set_window`, `set_length` (the timeline a player plays over
  until it enters its window, and the end of a window that states none;
  absent, the latest end of its instances); instance controls:
  `set_weight`, `set_start_offset`, `set_time_scale`, `set_rate`.
- Feedback: `player_states` reports each player's transport and instances.
- Baking: `bake` and `bake_with_derivatives` sample a loaded clip over a
  window at a frame rate (default 60), returning `Option<BakedAnimation>`.

Id-returning operations return `u32::MAX` for an unknown target or rejected
value. Removal returns a boolean, except `remove_instance` returns 1 or 0.
Reload returns false for invalid clips or unknown animation ids. A bake
returns none when the engine cannot bake, including invalid rates or too
many samples.

## Playback

The clock is integer nanoseconds. An explicit `time_ns` is the device time;
when absent, it is the previous time plus `dt_ns`, starting from zero. The
engine measures playback against that clock. Times enter the library as
signed `i64` nanoseconds. Keep unsigned device times and durations within
that range.

A new player plays immediately, looping at speed 1. Commands take effect at
the last step's device time. Before the first step they take effect at the
start of that step. `play_at` schedules a start at an absolute device time.
A negative speed plays backwards. The loop modes are `once`, `loop`,
`bounce` (turn once and end at the other bound), and `ping_pong` (turn at
every bound). `reversed` says a turn has made the player head against its
speed's sign; `ended` says it has reached its final bound.

`set_window` leaves the playhead where it stands. A player outside its window
plays toward it; once inside, its bounds apply. An instance maps player time
to clip time as `(playhead - offset) * rate`. Rates must be finite and
positive. `set_time_scale(ts)` sets `rate = 1 / ts`; state reports both
`rate` and `time_scale = 1 / rate`. A negative start offset begins the clip
earlier on the player timeline. Signed keypoints can precede clip time zero;
a positive offset can reach them while the player's playhead is nonnegative.

Numbers follow the resolved double precision Bézier curve. Booleans and
strings hold the keypoint starting their segment. At an interior keypoint's
exact instant, the preceding segment still holds; just after it, the new
value plays. The last keypoint plays at its own instant. Tracks hold their
first and last values outside their keypoint range.

Instances on one player blend numbers by weight; booleans and strings
replace the prior value. The first track on a target within a clip claims
it. Each player has its own output positions. `step` lists valued positions
in order. The array forms use `Unit` for positions with no value. Velocities
are `F64` in value units per second of the player's timeline, including the
instance rate, and `Unit` where a value or velocity is absent. They describe
the curve on the player timeline, independent of the player's transport speed.

Output revisions come from the engine's table rebuilds on structural edits;
transport and weight changes keep the revision. Consumers read `output_keys`
again when the returned revision changes.

Bakes sample clip-local time, including the first sample at or beyond the
requested end. The window defaults to zero through the clip's end. Each
track's values form an `ArrayValue`; empty tracks have empty samples.
`bake` leaves derivatives empty. `bake_with_derivatives` returns one
`Option(Some(F64))` per numeric sample in value units per second of clip-local
time, or `Option(None)` for booleans and strings.

## Building and testing

Use the workspace's pinned nightly toolchain:

```sh
cargo build -p arora-animation-module --target wasm32-wasip1
cargo run -p arora-animation-module --example header
cargo test -p arora-animation-module
cargo test -p arora-integration-tests --test animation
cargo clippy -p arora-animation-module --all-targets -- -D warnings
cargo fmt -p arora-animation-module
```

Module tests cover conversions, playback, transport, revisions, baking,
independent host registrations, declaration equality, and the literal ids
of the 25 retained functions and their parameters. The integration test
loads the built wasm guest and compares its calls with a host registration.
