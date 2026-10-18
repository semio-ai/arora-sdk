//! Records carried across the module boundary.
use arora_types::{value::Value, AroraType};

/// A clip of format 1.x: tracks at stamps that are fractions of `duration`
/// milliseconds, read through `arora_animation::clip1`.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000100", version = "1.1.0")]
pub struct AnimationClip {
    #[arora(id = "76697a69-6a00-0000-0100-000000000001")]
    pub name: String,
    #[arora(id = "76697a69-6a00-0000-0100-000000000002")]
    pub duration: u32,
    #[arora(id = "76697a69-6a00-0000-0100-000000000003")]
    pub tracks: Vec<AnimTrack>,
}
/// A 1.x track: the key it animates (`animatable_id`) and its keypoints.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000101", version = "1.1.0")]
pub struct AnimTrack {
    #[arora(id = "76697a69-6a00-0000-0101-000000000001")]
    pub id: String,
    #[arora(id = "76697a69-6a00-0000-0101-000000000002")]
    pub name: String,
    #[arora(id = "76697a69-6a00-0000-0101-000000000003")]
    pub animatable_id: String,
    #[arora(id = "76697a69-6a00-0000-0101-000000000004")]
    pub points: Vec<Keypoint>,
}
/// A 1.x keypoint: a stamp as a fraction of the clip's duration, a scalar
/// value, and zero or one CSS-style timing handle per side, normalized to its
/// segment.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000102", version = "1.1.0")]
pub struct Keypoint {
    #[arora(id = "76697a69-6a00-0000-0102-000000000001")]
    pub id: String,
    #[arora(id = "76697a69-6a00-0000-0102-000000000002")]
    pub stamp: f32,
    #[arora(id = "76697a69-6a00-0000-0102-000000000003", keyvalue)]
    pub value: Value,
    #[arora(id = "76697a69-6a00-0000-0102-000000000004")]
    pub transitions_in: Vec<TransitionHandle>,
    #[arora(id = "76697a69-6a00-0000-0102-000000000005")]
    pub transitions_out: Vec<TransitionHandle>,
}
/// A 1.x timing handle in normalized segment space: `x` time, `y` value.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000103")]
pub struct TransitionHandle {
    #[arora(id = "76697a69-6a00-0000-0103-000000000001")]
    pub x: f32,
    #[arora(id = "76697a69-6a00-0000-0103-000000000002")]
    pub y: f32,
}
/// One output position's value at a step: the track that first writes it,
/// the key it carries, and the value.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000110")]
pub struct TrackOutput {
    #[arora(id = "76697a69-6a00-0000-0110-000000000001")]
    pub track_id: String,
    #[arora(id = "76697a69-6a00-0000-0110-000000000002")]
    pub default_key: String,
    #[arora(id = "76697a69-6a00-0000-0110-000000000003", keyvalue)]
    pub value: Value,
}
/// A step's values by output position, and the revision of the table
/// `output_keys` lays them out by; `Unit` where no instance weighs.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000113")]
pub struct StepValues {
    #[arora(id = "76697a69-6a00-0000-0113-000000000001")]
    pub revision: u32,
    #[arora(id = "76697a69-6a00-0000-0113-000000000002", keyvalue)]
    pub values: Value,
}
/// The key of each output position, and the table's revision: per player in
/// creation order, each key in the order its instances first write it.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000114")]
pub struct OutputKeys {
    #[arora(id = "76697a69-6a00-0000-0114-000000000001")]
    pub revision: u32,
    #[arora(id = "76697a69-6a00-0000-0114-000000000002")]
    pub keys: Vec<String>,
}
/// One player as it stands: its state (`playing`, `paused`, `stopped`), its
/// playhead and length in nanoseconds of its timeline, its speed as set
/// (negative backwards), its name, its instances, its loop mode, whether the
/// loop mode stopped it at a bound (`ended`), its window, and whether it
/// heads against its speed's sign after a turn (`reversed`), and whether it
/// waits for the device time `play_at` gave it (`waiting`; its state says
/// `playing` meanwhile).
///
/// Record 1.3.0 adds `reversed` and `waiting` to 1.2.0.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000111", version = "1.3.0")]
pub struct PlayerState {
    #[arora(id = "76697a69-6a00-0000-0111-000000000001")]
    pub player: u32,
    #[arora(id = "76697a69-6a00-0000-0111-000000000002")]
    pub state: String,
    #[arora(id = "76697a69-6a00-0000-0111-000000000003")]
    pub time_ns: u64,
    #[arora(id = "76697a69-6a00-0000-0111-000000000004")]
    pub duration_ns: u64,
    #[arora(id = "76697a69-6a00-0000-0111-000000000005")]
    pub speed: f32,
    #[arora(id = "76697a69-6a00-0000-0111-000000000006")]
    pub name: String,
    #[arora(id = "76697a69-6a00-0000-0111-000000000007")]
    pub instances: Vec<InstanceState>,
    #[arora(id = "76697a69-6a00-0000-0111-000000000008")]
    pub loop_mode: String,
    #[arora(id = "76697a69-6a00-0000-0111-000000000009")]
    pub ended: bool,
    #[arora(id = "76697a69-6a00-0000-0111-00000000000a")]
    pub window_start_ns: u64,
    #[arora(id = "76697a69-6a00-0000-0111-00000000000b")]
    pub window_end_ns: Option<u64>,
    /// Heading against the speed's sign after a turn at a bound.
    #[arora(id = "76697a69-6a00-0000-0111-00000000000c")]
    pub reversed: bool,
    #[arora(id = "76697a69-6a00-0000-0111-00000000000d")]
    pub waiting: bool,
}
/// One instance on a player: the clip it plays, its weight, and its timing,
/// clip-local time = (playhead − `start_offset_ns`) × `rate`; `time_scale` is
/// `1 / rate`, the duration multiplier 1.x callers set.
///
/// Record 1.2.0 adds `rate` to 1.1.0.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000112", version = "1.2.0")]
pub struct InstanceState {
    #[arora(id = "76697a69-6a00-0000-0112-000000000001")]
    pub instance: u32,
    #[arora(id = "76697a69-6a00-0000-0112-000000000002")]
    pub anim: u32,
    #[arora(id = "76697a69-6a00-0000-0112-000000000003")]
    pub weight: f32,
    #[arora(id = "76697a69-6a00-0000-0112-000000000004")]
    pub start_offset_ns: i64,
    #[arora(id = "76697a69-6a00-0000-0112-000000000005")]
    pub time_scale: f32,
    /// Clip-local time = (playhead - start_offset_ns) * rate.
    #[arora(id = "76697a69-6a00-0000-0112-000000000006")]
    pub rate: f64,
}
/// A clip sampled at `frame_rate` over a window of its clip-local time, in
/// nanoseconds: samples from `start_ns` up to the first at or past `end_ns`.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000120")]
pub struct BakedAnimation {
    #[arora(id = "76697a69-6a00-0000-0120-000000000001")]
    pub frame_rate: f32,
    #[arora(id = "76697a69-6a00-0000-0120-000000000002")]
    pub start_ns: u64,
    #[arora(id = "76697a69-6a00-0000-0120-000000000003")]
    pub end_ns: u64,
    #[arora(id = "76697a69-6a00-0000-0120-000000000004")]
    pub tracks: Vec<BakedTrack>,
}
/// One track's samples: its key, its values (an `ArrayValue`), and from
/// `bake_with_derivatives` one `Option` of an `F64` velocity per sample, per
/// second of clip-local time (`None` for a boolean or string).
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000121")]
pub struct BakedTrack {
    #[arora(id = "76697a69-6a00-0000-0121-000000000001")]
    pub animatable_id: String,
    #[arora(id = "76697a69-6a00-0000-0121-000000000002", keyvalue)]
    pub values: Value,
    #[arora(id = "76697a69-6a00-0000-0121-000000000003", keyvalue)]
    pub derivatives: Value,
}

/// A clip of format 2.0, as `arora_animation::Clip` holds it: tracks of
/// keypoints at signed nanoseconds of clip-local time, and how long the clip
/// plays when nothing else bounds it (`length_ns`; absent, to its last
/// keypoint).
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000200", version = "2.0.0")]
pub struct Clip {
    #[arora(id = "76697a69-6a00-0000-0200-000000000001")]
    pub name: String,
    #[arora(id = "76697a69-6a00-0000-0200-000000000002")]
    pub length_ns: Option<i64>,
    #[arora(id = "76697a69-6a00-0000-0200-000000000003")]
    pub tracks: Vec<ClipTrack>,
}

/// A 2.0 track: the key it animates (`target`) and its keypoints, in any
/// order.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000201", version = "2.0.0")]
pub struct ClipTrack {
    #[arora(id = "76697a69-6a00-0000-0201-000000000001")]
    pub id: String,
    #[arora(id = "76697a69-6a00-0000-0201-000000000002")]
    pub name: String,
    #[arora(id = "76697a69-6a00-0000-0201-000000000003")]
    pub target: String,
    #[arora(id = "76697a69-6a00-0000-0201-000000000004")]
    pub keypoints: Vec<ClipKeypoint>,
}

/// A 2.0 keypoint: its signed time in nanoseconds, a scalar value, the
/// transition its author chose for each side (zero or one: none eases as
/// `cubic`), and the editor's pairing of its handles (`""`, `"paired"` or
/// `"free"`), which playback ignores.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000202", version = "2.0.0")]
pub struct ClipKeypoint {
    #[arora(id = "76697a69-6a00-0000-0202-000000000001")]
    pub id: String,
    #[arora(id = "76697a69-6a00-0000-0202-000000000002")]
    pub time_ns: i64,
    #[arora(id = "76697a69-6a00-0000-0202-000000000003", keyvalue)]
    pub value: Value,
    #[arora(id = "76697a69-6a00-0000-0202-000000000004")]
    pub arrive: Vec<ClipTransition>,
    #[arora(id = "76697a69-6a00-0000-0202-000000000005")]
    pub leave: Vec<ClipTransition>,
    #[arora(id = "76697a69-6a00-0000-0202-000000000006")]
    pub pairing: String,
}

/// A side's transition: `kind` is `"handle"`, an easing (`linear`, `sine`,
/// `quad`, `cubic`, `quart`, `quint`, `expo`, `circ`, `back`),
/// `"explicit-handles"` (the tangent the neighbours give) or
/// `"inferred-auto-clamped"`; a handle's control point is `dt_ns`
/// nanoseconds and `dv` value units from its keypoint.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000203", version = "2.0.0")]
pub struct ClipTransition {
    #[arora(id = "76697a69-6a00-0000-0203-000000000001")]
    pub kind: String,
    #[arora(id = "76697a69-6a00-0000-0203-000000000002")]
    pub dt_ns: f64,
    #[arora(id = "76697a69-6a00-0000-0203-000000000003")]
    pub dv: f64,
}

/// A step's values and velocities by output position, as `StepValues`; a
/// velocity is an `F64` in value units per second of the player's timeline,
/// `Unit` where there is none.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "76697a69-6a00-0000-0000-000000000115")]
pub struct StepSamples {
    #[arora(id = "76697a69-6a00-0000-0115-000000000001")]
    pub revision: u32,
    #[arora(id = "76697a69-6a00-0000-0115-000000000002", keyvalue)]
    pub values: Value,
    #[arora(id = "76697a69-6a00-0000-0115-000000000003", keyvalue)]
    pub velocities: Value,
}
