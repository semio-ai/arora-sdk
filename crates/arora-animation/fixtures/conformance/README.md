# Animation reference results

These cases define results the animation engine must reproduce. They are the
reference set Semio Studio generates from its own curve logic
(`packages/animation/conformance/` in semio_studio), copied here unchanged:
`tests/conformance.rs` evaluates every case through the crate.

They are regenerated in semio_studio, never edited here. A change to Studio's
handle resolution changes them there, and the copy here follows.

## Format

One file per animation and instance setting:

- `animation`: a stored Studio animation of format 2, its tracks embedded:
  stamps and handle times in milliseconds, transitions as Studio spells them.
- `instance`: `offset` in world milliseconds, `timescale` as a speed
  multiplier, and `active` (`false` means the instance is muted).
- `samples`: sorted `worldTimeMs`, and a `tracks` object keyed by track id.
  Each entry holds `value` and `velocity`. Velocity is value units per second
  of world time; `null` for strings and booleans. A muted instance, a muted
  track (`settings.muted`) or an empty track contributes nothing: both fields
  are `null`. A held number has velocity zero.

Numbers are JSON doubles, to be compared with a tolerance.

## Rules

1. A number segment's time is solved for the Bézier parameter to double
   precision, and its value evaluated on the curve. The easings use Studio's
   preset control points.
2. The last keypoint plays at its instant, booleans and strings included.
3. Strings and booleans hold the left value between keypoints. At an
   interior keypoint's exact instant they still hold the left segment's value;
   just after it they play that keypoint's value.
4. Handles resolve in local time by Studio's rules (presets, the two inferred
   tangents, explicit deltas, and the clamps). The curve maps to world time as
   `world = local / timescale + offset`: handle time deltas scale by
   `1 / timescale` like the keypoints' times; value deltas do not scale.
5. Negative local times are evaluated as they are, not clamped to 0. Before
   the first keypoint the first value holds, after the last the last.
6. A number's velocity is the curve's derivative `dy/du / dx/du`, per second
   of world time. At an interior keypoint it is the left segment's. At the
   first and last instants, the boundaries of the holds, it is the held
   value's zero. At a collapsed handle it is the first non-zero parametric
   derivative's ratio.
7. Each track is evaluated on its own: which track plays a target, looping,
   transport and blending are outside the set.

## Cases

The three fixture animations of Studio's tests, and one animation per rule:
strings, a boolean's final keypoint, asymmetric and clamped explicit handles,
keypoints before 0, large values at fractional stamps, both inferred
tangents, each of the nine easings, a single keypoint and an empty track. Each
is recorded at timescales 0.5, 1 and 2 with an offset of 137 ms, and two more
cases mute a track and an instance. Every case samples a 257-point grid
reaching past both ends, and every keypoint at its exact world instant and
0.001 ms either side.
