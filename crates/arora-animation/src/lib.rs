//! Keyframe animation for Arora.
//!
//! - [`clip`]: clip format 2.0, tracks of keypoints at signed nanoseconds of
//!   clip-local time with the transitions their author chose, and its JSON
//!   document form; [`clip1`] reads clip format 1.x into it.
//! - [`curve`]: a track compiled for exact evaluation, value and velocity,
//!   one time or a batch; [`resolve`] places each number segment's control
//!   points by the rules Semio Studio's editor draws them with.
//! - [`player`]: playback on a device clock: a window, looping and bouncing,
//!   a signed speed, and a start at a given instant, in integer nanoseconds
//!   that do not drift however long a player runs.
//! - [`engine`]: clips, players and their instances, stepped together,
//!   writing one value per key; and baking.
//!
//! The crate depends on no Arora runtime: the animation module
//! (`modules/animation` in arora-sdk) declares it as an Arora module, and
//! anything else — a browser page, a simulator — can evaluate a clip with it
//! directly.

pub mod clip;
pub mod clip1;
pub mod curve;
pub mod engine;
pub mod player;
pub mod resolve;
pub mod time;
pub mod value;

pub use clip::{Clip, ClipError, Easing, Keypoint, Pairing, Track, Transition, Transitions};
pub use curve::Curve;
pub use engine::{AnimId, Engine, InstId, PlayerId, Timing};
pub use player::{LoopMode, Player};
pub use time::Nanos;
pub use value::{Kind, Sample, Value};
