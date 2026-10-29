# arora-animation-module

The module `@arora/animation`: the animations and players of
[`arora-animation`](../../crates/arora-animation/readme.md) as functions a
behavior calls. A device loads it as a WebAssembly component from a module
directory, or a host links it in; either way, each loaded module keeps its own
animations and players.

| | |
|---|---|
| Package | `arora-animation-module` 2.0.0 |
| Module | `@arora/animation` 3.0.0: named `animation` in the `arora` scope, record path `arora.animation` |
| Module id | `76697a69-6a00-0000-0d00-000000000000` |

## Playing an animation from a behavior

A behavior loads an animation (`load_clip`, which answers its id), creates a
player (`create_player`: playing, looping, at speed 1), and puts the
animation on the player (`add_instance`). Then, at every step, it calls
`step_values` with the device's clock: the module moves its players and
returns one value per output position. `output_keys` names each position by
the target of the track that writes it.

The module writes no key. What it returns are values, and the behavior
chooses where they go: a behavior tree writes them to the store with
`scatter`, the targets as keys.

```text
sequence
  {.step} = step_values(dt_ns: {arora/dt}, time_ns: {arora/time})
  {.keys} = output_keys
  scatter(values: {.step}[.76697a69-6a00-0000-0113-000000000002], keys: {.keys}[.76697a69-6a00-0000-0114-000000000002], within: "rig")
```

`step_values` and `output_keys` are leaves the tree ticks at every step,
since stepping advances the players, and their results are tree-local
variables. The selections read the `values` field of `StepValues` and the
`keys` field of `OutputKeys`, by field id. `scatter` writes each value to the
key its target names, every target under `rig` here, and leaves a key as it
is where no instance plays. See the
[behavior tree's nodes](../../crates/arora-behavior-tree/docs/nodes.md) for
`scatter`.

## Loading the module

**Host-linked.** A host registers one state per device, so that devices in
one process each own their animations and players:

```rust
use arora_animation_module::{animation, exports, AnimationModule};
use arora_engine::module::HostModule;

let module = HostModule::from_exports(
    animation::ids::MODULE,
    exports(AnimationModule::new()),
);
// engine.register_module(module.id(), Box::new(module));
```

**As a guest.** The `cdylib` built for `wasm32-wasip2` is a component of the
`arora:module` world, with one state in its instance. A device loads it from
a **module directory**, which the `module-dir` feature's `module_dir::write`
and the `module_dir` example write:

```text
<dir>/
├── header.json            the declared header, naming the wasm-component executor
├── animation.wasm         the guest
└── records/               the module's record and the records of the types
                           its functions name, in arora-registry's layout
```

A device given the records (the `arora` runner reads `records/`;
[`arora-web`](../../crates/arora-web/readme.md) takes them with
`withRecords`) describes every function from the module's record, the
record-typed ones (`step_values`, `player_states`) included.

The functions are declared twice, with the same ids: as the contract
`AnimationContract`, whose methods take the state of one registration, and as
the module `animation`, whose functions act on the guest's state and give the
guest its entry points and its header (`animation::header`). A test holds the
two declarations equal. A call's arguments match by parameter id, in any
order; `animation::ids` holds the module, function and parameter ids.

## Functions

- **Loading.** `load_clip` and `reload_clip` take an animation of format 2.0
  (the record `Clip`); `load_animation` and `reload_animation` one of format
  1.x (`AnimationClip`). `create_player` creates a player; `add_instance` and
  `add_instance_with_weight` put an animation on it. `remove_instance`,
  `remove_player` and `unload_animation` undo them.
- **Stepping.** `step_values(dt_ns, time_ns?)` returns `StepValues`: the
  output table's revision and a value per position, `()` where no instance
  plays. `step_with_velocities` returns `StepSamples`, with velocities beside
  the values. `step` returns the positions that have a value as
  `TrackOutput`s, each with its track id and target. `output_keys` gives the
  target of each position, and the table's revision.
- **Transport.** `play`, `play_at` (a start at an absolute device time),
  `pause`, `stop`, `seek`, `set_speed`, `set_loop` (`once`, `loop`, `bounce`,
  `ping_pong`), `set_window`, `set_length` (the timeline a player plays over
  until it enters its window, and the end of a window that states none;
  absent, the latest end of its instances), `set_reversed` (the heading
  another player has after a turn at a bound, for a player started alongside
  it).
- **Instances.** `set_weight`, `set_start_offset`, `set_rate`,
  `set_time_scale` (`rate = 1 / time_scale`).
- **Feedback.** `player_states` reports each player's transport and
  instances as `PlayerState`s.
- **Baking.** `bake` and `bake_with_derivatives` sample a loaded animation
  over a window of its local time at a frame rate (60 by default), as a
  `BakedAnimation`.

A function answering an id answers `u32::MAX` for an unknown target or a
rejected value, such as a speed that is not finite or is faster than 2^20
times device time either way. A removal answers a boolean, but
`remove_instance` answers 1 or 0. A reload answers `false` for an invalid
animation or an unknown id. A bake answers none for an unknown animation, a
rate that is not finite and positive, or too many samples.

## Playback

The module keeps a clock of integer nanoseconds. A step's `time_ns` is the
device time, `arora/time` on a device; without it, the time is the previous
one plus `dt_ns`, from zero. Times are signed `i64` nanoseconds inside, so a
device time or a duration past `i64::MAX` is held at it.

A command takes effect at the last step's device time; before the first
step, at that step's start. An instance maps its player's playhead to the
animation's local time as `(playhead − start_offset) × rate`; a rate must be
finite and positive. A negative start offset starts the animation earlier on
the player's timeline, and a positive one reaches keypoints before 0 while
the playhead is not negative. `set_window` leaves the playhead where it
stands: a player outside its window plays toward it, and its bounds apply
once it is inside. `PlayerState`'s `reversed` says a turn has made the player
head against its speed's sign, `ended` that it reached its final bound.

The curves, steps, holds and blending are arora-animation's: numbers follow
the curve and blend by weight across a player's instances; booleans and
strings step and replace what came before; within an animation, the first
track on a target plays it. Velocities are `F64`, in value units per second
of the player's timeline, the instance's rate included and the player's
speed not. A bake's velocities are per second of local time. A 1.x track
whose numbers are all `F32` outputs `F32`; every other number track outputs
`F64`.

The revision changes when the output table is rebuilt, on a structural edit;
transport and weight changes keep it. A behavior reads `output_keys` again
when the revision a step returns changes.

## Records

Every record derives its schema and value form in Rust. A field holding a
`Value` of any type (a keypoint's value, a step's values) travels in the
self-describing value form.

| Record | Version | Holds |
|---|---|---|
| `Clip` | 2.0.0 | Name, optional signed `length_ns`, tracks |
| `ClipTrack` | 2.0.0 | Id, name, `target`, keypoints |
| `ClipKeypoint` | 2.0.0 | Id, signed `time_ns`, value, `arrive` and `leave` transitions, `pairing` |
| `ClipTransition` | 2.0.0 | `kind`, `dt_ns`, `dv` |
| `AnimationClip` | 1.2.0 | Name, duration in milliseconds, 1.x tracks |
| `AnimTrack` | 1.2.0 | Id, name, `animatable_id`, keypoints |
| `Keypoint` | 1.2.0 | Id, normalized stamp, value, normalized transition handles |
| `TransitionHandle` | 1.1.0 | Normalized `x` and `y` |
| `StepValues` | 1.1.0 | Output revision, values by position |
| `StepSamples` | 1.0.0 | Output revision, values and velocities by position |
| `OutputKeys` | 1.1.0 | Output revision, targets by position |
| `TrackOutput` | 1.1.0 | Track id, `default_key` (its target), value |
| `PlayerState` | 1.3.0 | Player id, state, position, length, speed, name, instances, loop mode, ended, window, reversed, waiting |
| `InstanceState` | 1.2.0 | Instance id, animation id, weight, signed start offset, time scale, rate |
| `BakedAnimation` | 1.1.0 | Frame rate, window in nanoseconds, tracks |
| `BakedTrack` | 1.1.0 | Target, values, and optional velocities |

A transition's `kind` is `handle` (its `dt_ns` and `dv` relative to its
keypoint, in nanoseconds and value units), an easing (`linear`, `sine`,
`quad`, `cubic`, `quart`, `quint`, `expo`, `circ`, `back`),
`explicit-handles`, or `inferred-auto-clamped`; a side has zero or one.
`pairing` is empty, `paired` or `free`. Both formats take numbers, booleans
and strings; a composite value, an unknown name, or a number or handle that
is not finite refuses the animation, and so does a track mixing kinds.
Integers read as `F64`. Format 1.x converts through arora-animation's
`clip1::ClipV1::to_clip`, keeping its handles and curve.

The module has the module, function, parameter and record ids of the module
`@vizij/animation` 2.0.0, so a caller of that module calls this one
unchanged. Its records live in the `arora` scope and `@vizij/animation`'s in
the `vizij` scope, and each record both carry is here at the next minor
above its version there, with the same fields, or for `PlayerState` and
`InstanceState` the fields that minor adds. A registry holds one record per
id and version, so both modules' records load into one registry, and a
reference to a 1.x record resolves to the one here.

## Building and testing

The workspace's pinned nightly toolchain builds the guest:

```sh
cargo build -p arora-animation-module --target wasm32-wasip2 --release
cargo run -p arora-animation-module --features module-dir --example module_dir -- \
    <dir> target/wasm32-wasip2/release/arora_animation_module.wasm
cargo test -p arora-animation-module
cargo test -p arora-integration-tests --test animation
```

The module's tests cover the conversions, playback, transport, revisions,
baking, independent host registrations, the two declarations' equality, and
the ids of the 25 functions `@vizij/animation` declares, with their
parameters. The integration tests load the built guest and compare its
answers with a host registration's, and load its module directory on a
device, which describes every function from the module's record.
