//! The engine: clips, players and the instances that put a clip on a
//! player, stepped together on a device clock.
//!
//! **Instances.** An instance plays one loaded clip on one player, at a
//! weight and a [`Timing`]: its clip-local time is
//! `(playhead − offset) × rate`, so a rate of 2 plays the clip twice as fast
//! and every handle with it. Before its first keypoint and after its last an
//! instance holds the value there.
//!
//! **Outputs.** A step writes one value per key and player. Within a clip,
//! the first track on a target is the one that plays it; a later track on
//! the same target is not played, and a track with no keypoint claims its
//! target and writes nothing. Within a player, the instances writing one key
//! blend: numbers by weight, their velocities likewise (an unbounded
//! velocity, at a tangent vertical in time, outweighs finite ones, and
//! opposite unbounded ones of equal weight cancel); a boolean or string
//! replaces what was written before it in the step. The output table —
//! per player in creation order, each key in the order its instances first
//! write it — changes only with a structural edit, which gives it a new
//! revision.
//!
//! **Commands** take effect at the device time of the last step (before
//! the first, at the start of the first): the clock the engine was last
//! moved to.

use crate::clip::Clip;
use crate::curve::Curve;
use crate::player::{LoopMode, Player, State, MAX_SPEED};
use crate::time::{Nanos, NANOS_PER_SECOND};
use crate::value::{Kind, Sample, Value};

/// A loaded clip's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnimId(pub u32);

/// A player's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub u32);

/// An instance's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstId(pub u32);

/// How an instance maps its player's timeline onto its clip:
/// `local = (playhead − offset) × rate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    /// Where on the player's timeline the clip's 0 falls, in nanoseconds.
    pub offset: Nanos,
    /// Clip-local nanoseconds per nanosecond of the player's timeline: a
    /// speed multiplier, finite and positive.
    pub rate: f64,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            offset: 0,
            rate: 1.0,
        }
    }
}

impl Timing {
    /// The clip-local time at playhead `t`.
    pub fn local(&self, t: f64) -> f64 {
        (t - self.offset as f64) * self.rate
    }

    /// The clip-local time at playhead `t` nanoseconds, the subtraction
    /// exact.
    pub fn local_at(&self, t: Nanos) -> f64 {
        (i128::from(t) - i128::from(self.offset)) as f64 * self.rate
    }

    /// Where on the timeline the clip-local time `local` falls, rounded up
    /// to a whole nanosecond.
    pub fn timeline_at(&self, local: Nanos) -> Nanos {
        let t = (local as f64 / self.rate).ceil() + self.offset as f64;
        t.clamp(i64::MIN as f64, i64::MAX as f64) as i64
    }
}

/// A loaded clip, compiled.
#[derive(Clone, Debug)]
struct Animation {
    id: AnimId,
    clip: Clip,
    curves: Vec<Curve>,
    /// Which tracks play: the first on each target.
    plays: Vec<bool>,
    /// Which tracks output single-precision numbers (a clip 1.x track whose
    /// keypoints are all `f32`).
    narrow: Vec<bool>,
}

impl Animation {
    fn new(id: AnimId, clip: Clip, narrow: Vec<bool>) -> Animation {
        let curves = clip.tracks.iter().map(Curve::new).collect();
        let mut claimed = std::collections::HashSet::new();
        let plays = clip
            .tracks
            .iter()
            .map(|t| claimed.insert(t.target.clone()))
            .collect();
        let narrow = (0..clip.tracks.len())
            .map(|i| narrow.get(i).copied().unwrap_or(false))
            .collect();
        Animation {
            id,
            clip,
            curves,
            plays,
            narrow,
        }
    }

    /// The tracks that play and write: claimed, with keypoints.
    fn played(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.curves.len()).filter(|&i| self.plays[i] && self.curves[i].kind().is_some())
    }
}

/// An instance of a clip on a player.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    pub id: InstId,
    pub anim: AnimId,
    pub weight: f64,
    pub timing: Timing,
}

#[derive(Clone, Debug)]
struct PlayerSlot {
    id: PlayerId,
    name: String,
    player: Player,
    /// The timeline's length as set; none, the latest end of the instances.
    length: Option<Nanos>,
    instances: Vec<Instance>,
    /// Each played track of each instance: its position in the output table.
    routes: Vec<Vec<usize>>,
}

/// One position of a step's output.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// The value written to the position's key this step; `None` when no
    /// instance weighed on it.
    pub sample: Option<Sample>,
    /// Whether every contribution was a single-precision track's.
    pub narrow: bool,
}

/// A player as it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
    pub state: State,
    pub position: Nanos,
    /// The player's length: as set, else the latest end of its instances
    /// on its timeline.
    pub length: Nanos,
    pub speed: f64,
    pub reversed: bool,
    pub mode: LoopMode,
    pub ended: bool,
    /// Whether it waits for the device time `play_at` gave it.
    pub waiting: bool,
    pub window: (Nanos, Option<Nanos>),
    pub instances: Vec<Instance>,
}

/// A clip sampled at a fixed rate over a window of its clip-local time.
#[derive(Clone, Debug, PartialEq)]
pub struct Bake {
    /// Samples per second.
    pub rate: f64,
    /// The first sample's clip-local time, in nanoseconds.
    pub start: Nanos,
    /// The window's end, in nanoseconds: the last sample is the first at or
    /// past it.
    pub end: Nanos,
    /// One entry per track, in the clip's order: its target and its
    /// samples (none for a track with no keypoint).
    pub tracks: Vec<(String, Vec<Sample>)>,
}

/// The most samples one bake takes, over all its tracks.
pub const MAX_BAKE_SAMPLES: usize = 1 << 20;

/// A buffered command, held until the engine knows the device time.
#[derive(Clone, Debug)]
enum Command {
    Play,
    PlayAt(Nanos),
    Pause,
    Stop,
    Seek(Nanos),
    Speed(f64),
    Window(Nanos, Option<Nanos>),
    Reversed(bool),
}

/// Clips, players and instances, stepped together.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    next_anim: u32,
    next_player: u32,
    next_instance: u32,
    animations: Vec<Animation>,
    players: Vec<PlayerSlot>,
    /// The device time of the last step.
    clock: Option<Nanos>,
    /// Commands given before the first step, with their players.
    early: Vec<(PlayerId, Command)>,
    /// The key of each output position.
    keys: Vec<String>,
    /// The id of the track that first writes each output position.
    tracks: Vec<String>,
    revision: u32,
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Loads `clip` and returns its id. `narrow` marks the tracks, by
    /// index, that output single-precision numbers.
    pub fn load(&mut self, clip: Clip, narrow: Vec<bool>) -> AnimId {
        let id = AnimId(self.next_anim);
        self.next_anim += 1;
        self.animations.push(Animation::new(id, clip, narrow));
        id
    }

    /// Replaces loaded clip `anim` with `clip`, keeping its id: its
    /// instances stay on their players and play the new tracks from the next
    /// step. `false`, changing nothing, when `anim` is not loaded.
    pub fn reload(&mut self, anim: AnimId, clip: Clip, narrow: Vec<bool>) -> bool {
        let Some(slot) = self.animations.iter_mut().find(|a| a.id == anim) else {
            return false;
        };
        *slot = Animation::new(anim, clip, narrow);
        self.restructure();
        true
    }

    /// Unloads `anim` and removes every instance of it.
    pub fn unload(&mut self, anim: AnimId) -> bool {
        let before = self.animations.len();
        self.animations.retain(|a| a.id != anim);
        if self.animations.len() == before {
            return false;
        }
        for slot in &mut self.players {
            slot.instances.retain(|i| i.anim != anim);
        }
        self.restructure();
        true
    }

    /// The loaded clip `anim`.
    pub fn clip(&self, anim: AnimId) -> Option<&Clip> {
        self.animation(anim).map(|a| &a.clip)
    }

    /// Creates a player, paused at 0 and playing once, and returns its id.
    pub fn create_player(&mut self, name: &str) -> PlayerId {
        let id = PlayerId(self.next_player);
        self.next_player += 1;
        self.players.push(PlayerSlot {
            id,
            name: name.to_string(),
            player: Player::new(),
            length: None,
            instances: Vec::new(),
            routes: Vec::new(),
        });
        self.restructure();
        id
    }

    /// Removes a player and its instances; the clips stay loaded.
    pub fn remove_player(&mut self, player: PlayerId) -> bool {
        let before = self.players.len();
        self.players.retain(|p| p.id != player);
        self.early.retain(|(p, _)| *p != player);
        let removed = self.players.len() != before;
        if removed {
            self.restructure();
        }
        removed
    }

    /// Puts an instance of `anim` on `player`; `None` when either does not
    /// exist, or the weight or timing is not valid (a weight finite and not
    /// negative, a rate finite and positive).
    pub fn add_instance(
        &mut self,
        player: PlayerId,
        anim: AnimId,
        weight: f64,
        timing: Timing,
    ) -> Option<InstId> {
        if self.animation(anim).is_none() || !valid_weight(weight) || !valid_rate(timing.rate) {
            return None;
        }
        let id = InstId(self.next_instance);
        let slot = self.players.iter_mut().find(|p| p.id == player)?;
        self.next_instance += 1;
        slot.instances.push(Instance {
            id,
            anim,
            weight,
            timing,
        });
        self.restructure();
        Some(id)
    }

    /// Removes an instance from its player.
    pub fn remove_instance(&mut self, player: PlayerId, instance: InstId) -> bool {
        let Some(slot) = self.players.iter_mut().find(|p| p.id == player) else {
            return false;
        };
        let before = slot.instances.len();
        slot.instances.retain(|i| i.id != instance);
        let removed = slot.instances.len() != before;
        if removed {
            self.restructure();
        }
        removed
    }

    /// Sets an instance's weight; `false` when it does not exist or the
    /// weight is not finite and non-negative.
    pub fn set_weight(&mut self, player: PlayerId, instance: InstId, weight: f64) -> bool {
        if !valid_weight(weight) {
            return false;
        }
        self.instance_mut(player, instance)
            .map(|i| i.weight = weight)
            .is_some()
    }

    /// Sets an instance's timing; `false` when it does not exist or the
    /// rate is not finite and positive.
    pub fn set_timing(&mut self, player: PlayerId, instance: InstId, timing: Timing) -> bool {
        if !valid_rate(timing.rate) {
            return false;
        }
        self.instance_mut(player, instance)
            .map(|i| i.timing = timing)
            .is_some()
    }

    /// The instance `instance` of `player`.
    pub fn instance(&self, player: PlayerId, instance: InstId) -> Option<&Instance> {
        self.slot(player)?
            .instances
            .iter()
            .find(|i| i.id == instance)
    }

    pub fn play(&mut self, player: PlayerId) -> bool {
        self.command(player, Command::Play)
    }

    /// Starts `player` at device time `start` (see [`Player::play_at`]).
    pub fn play_at(&mut self, player: PlayerId, start: Nanos) -> bool {
        self.command(player, Command::PlayAt(start))
    }

    pub fn pause(&mut self, player: PlayerId) -> bool {
        self.command(player, Command::Pause)
    }

    pub fn stop(&mut self, player: PlayerId) -> bool {
        self.command(player, Command::Stop)
    }

    pub fn seek(&mut self, player: PlayerId, position: Nanos) -> bool {
        self.command(player, Command::Seek(position))
    }

    /// Sets a player's speed; `false` when it does not exist or the speed is
    /// not finite or faster than [`MAX_SPEED`] either way.
    pub fn set_speed(&mut self, player: PlayerId, speed: f64) -> bool {
        speed.is_finite() && speed.abs() <= MAX_SPEED && self.command(player, Command::Speed(speed))
    }

    pub fn set_window(&mut self, player: PlayerId, start: Nanos, end: Option<Nanos>) -> bool {
        self.command(player, Command::Window(start, end))
    }

    /// Heads `player` against its speed's sign, as after a turn at a bound,
    /// or with it ([`Player::set_reversed`]): how a player started late takes
    /// the heading of one it is to play alongside.
    pub fn set_reversed(&mut self, player: PlayerId, reversed: bool) -> bool {
        self.command(player, Command::Reversed(reversed))
    }

    /// Sets the length of `player`'s timeline, the bounds it plays within
    /// until it enters its window and the end of a window that states none;
    /// `None` returns it to the latest end of its instances. A host whose
    /// timeline is longer than the clips on it (Studio's, at least a second
    /// and as long as its viewport) gives every player that timeline.
    pub fn set_length(&mut self, player: PlayerId, length: Option<Nanos>) -> bool {
        let Some(slot) = self.players.iter_mut().find(|p| p.id == player) else {
            return false;
        };
        slot.length = length.map(|l| l.max(0));
        true
    }

    pub fn set_mode(&mut self, player: PlayerId, mode: LoopMode) -> bool {
        let Some(slot) = self.players.iter_mut().find(|p| p.id == player) else {
            return false;
        };
        slot.player.set_mode(mode);
        true
    }

    /// The players, in creation order.
    pub fn players(&self) -> Vec<PlayerInfo> {
        self.players
            .iter()
            .map(|slot| PlayerInfo {
                id: slot.id,
                name: slot.name.clone(),
                state: slot.player.state(),
                position: slot.player.position(),
                length: self.length(slot),
                speed: slot.player.speed(),
                reversed: slot.player.reversed(),
                mode: slot.player.mode(),
                ended: slot.player.ended(),
                waiting: slot.player.waiting(),
                window: slot.player.window(),
                instances: slot.instances.clone(),
            })
            .collect()
    }

    /// The key of each output position, and the table's revision.
    pub fn output_keys(&self) -> (u32, &[String]) {
        (self.revision, &self.keys)
    }

    /// The id of the track that first writes each output position: its
    /// player's first instance writing the key, that clip's track on it.
    pub fn output_tracks(&self) -> &[String] {
        &self.tracks
    }

    /// The device time of the last step.
    pub fn clock(&self) -> Option<Nanos> {
        self.clock
    }

    /// Moves every player to device time `now` and returns the value of each
    /// output position there (see [`Engine::output_keys`]).
    pub fn step(&mut self, now: Nanos, dt: Nanos) -> Vec<Output> {
        if self.clock.is_none() {
            let start = now.saturating_sub(dt);
            self.clock = Some(start);
            for (player, command) in std::mem::take(&mut self.early) {
                self.apply(player, command, start);
            }
        }
        self.clock = Some(now);
        let lengths: Vec<Nanos> = self.players.iter().map(|s| self.length(s)).collect();
        for (slot, length) in self.players.iter_mut().zip(lengths) {
            slot.player.advance(now, length);
        }
        self.outputs()
    }

    /// The value of each output position where the players now stand.
    pub fn outputs(&self) -> Vec<Output> {
        let mut acc: Vec<Accumulator> = vec![Accumulator::Empty; self.keys.len()];
        for slot in &self.players {
            let position = slot.player.position();
            for (instance, routes) in slot.instances.iter().zip(&slot.routes) {
                if instance.weight <= 0.0 {
                    continue;
                }
                let Some(animation) = self.animation(instance.anim) else {
                    continue;
                };
                let local = instance.timing.local_at(position);
                for (track, &route) in animation.played().zip(routes) {
                    let Some(sample) = animation.curves[track].sample(local) else {
                        continue;
                    };
                    acc[route].add(
                        sample,
                        instance.weight,
                        instance.timing.rate,
                        animation.narrow[track],
                    );
                }
            }
        }
        acc.into_iter().map(Accumulator::finish).collect()
    }

    /// Bakes `anim` at `rate` samples per second over `[start, end]` of its
    /// clip-local time (its whole clip, from 0 to its end, by default):
    /// samples from `start` up to the first at or past `end`. `None` when
    /// `anim` is not loaded, the rate is not finite and positive, or the bake
    /// would take more than [`MAX_BAKE_SAMPLES`].
    pub fn bake(
        &self,
        anim: AnimId,
        rate: f64,
        start: Option<Nanos>,
        end: Option<Nanos>,
    ) -> Option<Bake> {
        let animation = self.animation(anim)?;
        if !(rate.is_finite() && rate > 0.0) {
            return None;
        }
        let start = start.unwrap_or(0);
        let end = end.unwrap_or_else(|| animation.clip.end()).max(start);
        let period = NANOS_PER_SECOND as f64 / rate;
        let frames = bake_frames(start, end, period)?;
        let tracks_with_samples = animation
            .curves
            .iter()
            .filter(|c| c.kind().is_some())
            .count();
        if frames.saturating_mul(tracks_with_samples) > MAX_BAKE_SAMPLES {
            return None;
        }
        let times: Vec<f64> = (0..frames)
            .map(|i| start as f64 + i as f64 * period)
            .collect();
        let tracks = animation
            .clip
            .tracks
            .iter()
            .zip(&animation.curves)
            .map(|(track, curve)| {
                let mut samples = Vec::new();
                curve.sample_many(&times, &mut samples);
                (track.target.clone(), samples)
            })
            .collect();
        Some(Bake {
            rate,
            start,
            end,
            tracks,
        })
    }

    /// The compiled tracks of loaded clip `anim`, in its order.
    pub fn curves(&self, anim: AnimId) -> Option<&[Curve]> {
        self.animation(anim).map(|a| a.curves.as_slice())
    }

    fn command(&mut self, player: PlayerId, command: Command) -> bool {
        if self.slot(player).is_none() {
            return false;
        }
        match self.clock {
            Some(now) => self.apply(player, command, now),
            None => self.early.push((player, command)),
        }
        true
    }

    fn apply(&mut self, player: PlayerId, command: Command, at: Nanos) {
        let Some(index) = self.players.iter().position(|p| p.id == player) else {
            return;
        };
        let length = self.length(&self.players[index]);
        let p = &mut self.players[index].player;
        match command {
            Command::Play => p.play(at, length),
            Command::PlayAt(start) => p.play_at(at, start, length),
            Command::Pause => p.pause(at, length),
            Command::Stop => p.stop(at, length),
            Command::Seek(position) => p.seek(at, position, length),
            Command::Speed(speed) => p.set_speed(at, speed, length),
            Command::Window(start, end) => p.set_window(at, start, end, length),
            Command::Reversed(reversed) => p.set_reversed(at, reversed, length),
        }
    }

    /// A player's length: as set, else the latest end of its instances on
    /// its timeline, and never before 0.
    fn length(&self, slot: &PlayerSlot) -> Nanos {
        if let Some(length) = slot.length {
            return length;
        }
        slot.instances
            .iter()
            .filter_map(|i| {
                let end = self.animation(i.anim)?.clip.end();
                Some(i.timing.timeline_at(end))
            })
            .max()
            .unwrap_or(0)
            .max(0)
    }

    fn animation(&self, anim: AnimId) -> Option<&Animation> {
        self.animations.iter().find(|a| a.id == anim)
    }

    fn slot(&self, player: PlayerId) -> Option<&PlayerSlot> {
        self.players.iter().find(|p| p.id == player)
    }

    fn instance_mut(&mut self, player: PlayerId, instance: InstId) -> Option<&mut Instance> {
        self.players
            .iter_mut()
            .find(|p| p.id == player)?
            .instances
            .iter_mut()
            .find(|i| i.id == instance)
    }

    /// Rebuilds the output table after a structural edit, under a new
    /// revision.
    fn restructure(&mut self) {
        let mut keys = Vec::new();
        let mut tracks = Vec::new();
        let animations = &self.animations;
        for slot in &mut self.players {
            let first = keys.len();
            slot.routes = slot
                .instances
                .iter()
                .map(|instance| {
                    let Some(animation) = animations.iter().find(|a| a.id == instance.anim) else {
                        return Vec::new();
                    };
                    animation
                        .played()
                        .map(|track| {
                            let track = &animation.clip.tracks[track];
                            match keys[first..].iter().position(|k| *k == track.target) {
                                Some(at) => first + at,
                                None => {
                                    keys.push(track.target.clone());
                                    tracks.push(track.id.clone());
                                    keys.len() - 1
                                }
                            }
                        })
                        .collect()
                })
                .collect();
        }
        self.keys = keys;
        self.tracks = tracks;
        self.revision = self.revision.wrapping_add(1);
    }
}

/// How many samples a bake takes from `start` to `end` at `period`
/// nanoseconds apart: up to the first at or past `end`.
fn bake_frames(start: Nanos, end: Nanos, period: f64) -> Option<usize> {
    let span = (end - start) as f64;
    let mut frames = (span / period).floor();
    if !frames.is_finite() || frames >= MAX_BAKE_SAMPLES as f64 {
        return None;
    }
    // The floor of a quotient can land a sample short of `end` or on a
    // sample past it: settle it on the times the samples are taken at.
    while start as f64 + frames * period < end as f64 {
        frames += 1.0;
    }
    while frames > 0.0 && start as f64 + (frames - 1.0) * period >= end as f64 {
        frames -= 1.0;
    }
    Some(frames as usize + 1)
}

fn valid_weight(weight: f64) -> bool {
    weight.is_finite() && weight >= 0.0
}

fn valid_rate(rate: f64) -> bool {
    rate.is_finite() && rate > 0.0
}

/// One output position's contributions in a step.
#[derive(Clone, Debug)]
enum Accumulator {
    Empty,
    Numbers {
        sum: f64,
        /// The weighted sum of the finite velocities.
        velocity: f64,
        /// The weight of the contributions whose velocity is unbounded
        /// upward, and downward: a tangent vertical in time.
        unbounded: (f64, f64),
        weight: f64,
        narrow: bool,
    },
    Step {
        value: Value,
        narrow: bool,
    },
}

impl Accumulator {
    /// Adds a track's sample at `weight`; `rate` turns its clip-local
    /// velocity into one per second of the player's timeline.
    fn add(&mut self, sample: Sample, weight: f64, rate: f64, narrow: bool) {
        match (sample.value.kind(), &mut *self) {
            (
                Kind::Number,
                Accumulator::Numbers {
                    sum,
                    velocity,
                    unbounded,
                    weight: total,
                    narrow: all_narrow,
                },
            ) => {
                *sum += weight * sample.value.as_number().unwrap_or(0.0);
                let v = sample.velocity.unwrap_or(0.0) * rate;
                if v == f64::INFINITY {
                    unbounded.0 += weight;
                } else if v == f64::NEG_INFINITY {
                    unbounded.1 += weight;
                } else {
                    *velocity += weight * v;
                }
                *total += weight;
                *all_narrow &= narrow;
            }
            (Kind::Number, _) => {
                *self = Accumulator::Numbers {
                    sum: 0.0,
                    velocity: 0.0,
                    unbounded: (0.0, 0.0),
                    weight: 0.0,
                    narrow: true,
                };
                self.add(sample, weight, rate, narrow);
            }
            _ => {
                *self = Accumulator::Step {
                    value: sample.value,
                    narrow,
                };
            }
        }
    }

    fn finish(self) -> Output {
        match self {
            Accumulator::Empty => Output {
                sample: None,
                narrow: false,
            },
            Accumulator::Numbers {
                sum,
                velocity,
                unbounded: (up, down),
                weight,
                narrow,
            } => Output {
                sample: Some(Sample {
                    value: Value::Number(sum / weight),
                    // Unbounded velocities outweigh finite ones; opposite
                    // ones of equal weight cancel.
                    velocity: Some(if up > down {
                        f64::INFINITY
                    } else if down > up {
                        f64::NEG_INFINITY
                    } else {
                        velocity / weight
                    }),
                }),
                narrow,
            },
            Accumulator::Step { value, narrow } => Output {
                sample: Some(Sample {
                    value,
                    velocity: None,
                }),
                narrow,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_clip() -> Clip {
        Clip::from_json(
            r#"{"tracks":[
              {"id":"a","animatableId":"x","points":[{"id":"0","timeNs":0,"value":0,"transitions":{"out":"linear"}},{"id":"1","timeNs":1000000000,"value":10,"transitions":{"in":"linear"}}]},
              {"id":"b","animatableId":"x","points":[{"id":"0","timeNs":0,"value":99}]},
              {"id":"c","animatableId":"y","points":[]},
              {"id":"d","animatableId":"y","points":[{"id":"0","timeNs":0,"value":"no"}]},
              {"id":"e","animatableId":"z","points":[{"id":"0","timeNs":0,"value":false},{"id":"1","timeNs":500000000,"value":true},{"id":"2","timeNs":1000000000,"value":false}]}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn the_first_track_on_a_target_plays_it_and_an_empty_one_claims_it() {
        let mut engine = Engine::new();
        let anim = engine.load(ramp_clip(), vec![]);
        let player = engine.create_player("p");
        engine
            .add_instance(player, anim, 1.0, Timing::default())
            .unwrap();
        assert_eq!(engine.output_keys().1, ["x", "z"]);
        engine.play(player);
        let out = engine.step(500_000_000, 500_000_000);
        let x = out[0].sample.as_ref().unwrap().value.as_number().unwrap();
        assert!((x - 5.0).abs() < 1e-12, "{x}");
        assert_eq!(out[1].sample.as_ref().unwrap().value, Value::Bool(false));
        let out = engine.step(500_000_001, 1);
        assert_eq!(out[1].sample.as_ref().unwrap().value, Value::Bool(true));
    }

    #[test]
    fn an_instances_rate_scales_its_clip_and_its_velocity() {
        let mut engine = Engine::new();
        let anim = engine.load(ramp_clip(), vec![]);
        let player = engine.create_player("p");
        let timing = Timing {
            offset: 100_000_000,
            rate: 2.0,
        };
        engine.add_instance(player, anim, 1.0, timing).unwrap();
        assert_eq!(engine.players()[0].length, 600_000_000);
        engine.play(player);
        let out = engine.step(350_000_000, 350_000_000);
        let sample = out[0].sample.as_ref().unwrap();
        assert!((sample.value.as_number().unwrap() - 5.0).abs() < 1e-9);
        assert!((sample.velocity.unwrap() - 20.0).abs() < 1e-6);
    }

    #[test]
    fn a_length_set_bounds_the_timeline_beyond_the_clips() {
        let mut engine = Engine::new();
        let anim = engine.load(ramp_clip(), vec![]);
        let player = engine.create_player("p");
        engine
            .add_instance(player, anim, 1.0, Timing::default())
            .unwrap();
        engine.set_mode(player, LoopMode::Loop);
        assert!(engine.set_length(player, Some(3_000_000_000)));
        engine.play(player);
        engine.step(0, 0);
        engine.step(2_500_000_000, 2_500_000_000);
        assert_eq!(engine.players()[0].position, 2_500_000_000);
        assert_eq!(engine.players()[0].length, 3_000_000_000);
        engine.set_length(player, None);
        assert_eq!(engine.players()[0].length, 1_000_000_000);
        assert!(!engine.set_length(PlayerId(99), None));
    }

    /// A standalone clip's length carries playback past its last keypoint;
    /// a window overrides that end, and an absent length uses the keypoint.
    #[test]
    fn a_clips_length_is_its_natural_end() {
        let json = r#"{"lengthNs":3000000000,"tracks":[
            {"id":"t","animatableId":"x","points":[
                {"id":"a","timeNs":0,"value":0},
                {"id":"b","timeNs":1000000000,"value":1}
            ]}
        ]}"#;
        let without_length = json.replace("\"lengthNs\":3000000000,", "");
        for (clip, window_end, end) in [
            (json, None, 3_000_000_000),
            (json, Some(2_000_000_000), 2_000_000_000),
            (without_length.as_str(), None, 1_000_000_000),
        ] {
            let mut engine = Engine::new();
            let anim = engine.load(Clip::from_json(clip).unwrap(), vec![]);
            let player = engine.create_player("p");
            engine
                .add_instance(player, anim, 1.0, Timing::default())
                .unwrap();
            engine.set_mode(player, LoopMode::Once);
            if let Some(window_end) = window_end {
                engine.set_window(player, 0, Some(window_end));
            }
            engine.play(player);
            engine.step(0, 0);
            for (now, dt, position, ended) in [
                (end - 1, end - 1, end - 1, false),
                (end, 1, end, true),
                (end + 1_000_000_000, 1_000_000_000, end, true),
            ] {
                engine.step(now, dt);
                let info = &engine.players()[0];
                assert_eq!(
                    (info.position, info.ended),
                    (position, ended),
                    "end {end}, window {window_end:?}, time {now}"
                );
            }
        }
    }

    #[test]
    fn opposite_unbounded_velocities_of_equal_weight_cancel_in_a_blend() {
        let vertical = |rising: bool| {
            let (from, to) = if rising { (0.0, 1.0) } else { (1.0, 0.0) };
            let mut points = vec![
                crate::Keypoint {
                    id: "a".into(),
                    time: 0,
                    value: Value::Number(from),
                    transitions: Default::default(),
                },
                crate::Keypoint {
                    id: "b".into(),
                    time: 1_000_000_000,
                    value: Value::Number(to),
                    transitions: Default::default(),
                },
            ];
            points[0].transitions.leave = Some(crate::Transition::Handle { dt: 1e9, dv: 0.0 });
            points[1].transitions.arrive = Some(crate::Transition::Handle { dt: -1e9, dv: 0.0 });
            Clip {
                tracks: vec![crate::Track {
                    id: "t".into(),
                    target: "x".into(),
                    keypoints: points,
                    ..crate::Track::default()
                }],
                ..Clip::default()
            }
        };
        let blend = |weights: [f64; 2]| {
            let mut engine = Engine::new();
            let player = engine.create_player("p");
            for (rising, weight) in [(true, weights[0]), (false, weights[1])] {
                let anim = engine.load(vertical(rising), vec![]);
                engine.add_instance(
                    player,
                    anim,
                    weight,
                    Timing {
                        offset: 0,
                        rate: 1.0,
                    },
                );
            }
            engine.step(0, 0);
            engine.seek(player, 500_000_000);
            engine.step(0, 0);
            let sample = engine.outputs()[0].sample.clone().unwrap();
            sample.velocity.unwrap()
        };
        assert_eq!(blend([1.0, 1.0]), 0.0);
        assert_eq!(blend([2.0, 1.0]), f64::INFINITY);
        assert_eq!(blend([1.0, 3.0]), f64::NEG_INFINITY);
        assert!(!Engine::new().set_speed(PlayerId(0), MAX_SPEED * 2.0));
        let mut engine = Engine::new();
        let player = engine.create_player("p");
        assert!(engine.set_speed(player, -MAX_SPEED));
        assert!(!engine.set_speed(player, MAX_SPEED * 1.5));
    }

    #[test]
    fn instances_on_one_key_blend_by_weight() {
        let mut engine = Engine::new();
        let anim = engine.load(ramp_clip(), vec![]);
        let hold = engine.load(
            Clip::from_json(r#"{"tracks":[{"id":"h","animatableId":"x","points":[{"id":"0","timeNs":0,"value":100}]}]}"#).unwrap(),
            vec![],
        );
        let player = engine.create_player("p");
        engine
            .add_instance(player, anim, 1.0, Timing::default())
            .unwrap();
        let held = engine
            .add_instance(player, hold, 3.0, Timing::default())
            .unwrap();
        let out = engine.step(0, 0);
        assert_eq!(out[0].sample.as_ref().unwrap().value, Value::Number(75.0));
        // A second instance of the ramp at twice the rate: velocities blend
        // per second of the player's timeline, each scaled by its rate.
        let fast = engine
            .add_instance(
                player,
                anim,
                1.0,
                Timing {
                    offset: 0,
                    rate: 2.0,
                },
            )
            .unwrap();
        engine.play(player);
        let out = engine.step(100_000_000, 100_000_000);
        let velocity = out[0].sample.as_ref().unwrap().velocity.unwrap();
        let slow = engine.curves(anim).unwrap()[0]
            .sample(1e8)
            .unwrap()
            .velocity
            .unwrap();
        let quick = engine.curves(anim).unwrap()[0]
            .sample(2e8)
            .unwrap()
            .velocity
            .unwrap();
        assert!(
            (velocity - (slow + 2.0 * quick) / 5.0).abs() < 1e-9,
            "{velocity}"
        );
        assert!(engine.remove_instance(player, fast));
        engine.set_weight(player, held, 0.0);
        let out = engine.outputs();
        let x = out[0].sample.as_ref().unwrap().value.as_number().unwrap();
        assert!((x - 1.0).abs() < 1e-12, "the ramp alone, at 100 ms: {x}");
        assert!(!engine.set_weight(player, held, f64::NAN));
    }

    #[test]
    fn a_structural_edit_gives_the_output_table_a_new_revision() {
        let mut engine = Engine::new();
        let anim = engine.load(ramp_clip(), vec![]);
        let player = engine.create_player("p");
        let (r0, _) = engine.output_keys();
        let inst = engine
            .add_instance(player, anim, 1.0, Timing::default())
            .unwrap();
        let (r1, keys) = engine.output_keys();
        assert_ne!(r0, r1);
        assert_eq!(keys.len(), 2);
        engine.step(0, 0);
        assert_eq!(engine.output_keys().0, r1, "a step is not an edit");
        assert!(engine.remove_instance(player, inst));
        assert!(engine.output_keys().1.is_empty());
        assert!(engine.unload(anim));
        assert!(!engine.unload(anim));
    }

    #[test]
    fn a_bake_takes_its_samples_up_to_the_first_at_or_past_its_end() {
        let mut engine = Engine::new();
        let anim = engine.load(ramp_clip(), vec![]);
        let frames = |rate: f64, start: Option<Nanos>, end: Option<Nanos>| {
            engine.bake(anim, rate, start, end).unwrap().tracks[0]
                .1
                .len()
        };
        assert_eq!(frames(100.0, None, Some(70_000_000)), 8);
        assert_eq!(frames(100.0, None, Some(550_000_000)), 56);
        assert_eq!(frames(25.0, None, Some(280_000_000)), 8);
        assert_eq!(frames(100.0, None, Some(150_000_000)), 16);
        assert_eq!(frames(30.0, None, Some(8_100_000_000)), 244);
        assert_eq!(frames(1.0, None, Some(3_000_000_001)), 5);
        assert_eq!(frames(1.0, None, Some(3_000_000_000)), 4);
        assert_eq!(frames(60.0, Some(2_000_000_000), Some(2_000_000_000)), 1);
        assert_eq!(frames(100.0, None, None), 101);
        // A track with no keypoint has no samples.
        assert!(engine.bake(anim, 10.0, None, None).unwrap().tracks[2]
            .1
            .is_empty());
        assert!(engine.bake(anim, 0.0, None, None).is_none());
        assert!(engine.bake(anim, 1e9, None, Some(10_000_000_000)).is_none());
    }

    #[test]
    fn a_step_equals_the_bake_at_its_time() {
        let mut engine = Engine::new();
        let anim = engine.load(ramp_clip(), vec![]);
        let player = engine.create_player("p");
        engine
            .add_instance(player, anim, 1.0, Timing::default())
            .unwrap();
        engine.play(player);
        let bake = engine.bake(anim, 100.0, None, None).unwrap();
        let dt = 10_000_000;
        for (n, frame) in bake.tracks[0].1.iter().enumerate() {
            let out = engine.step(n as Nanos * dt, if n == 0 { 0 } else { dt });
            assert_eq!(
                out[0].sample.as_ref().unwrap().value,
                frame.value,
                "frame {n}"
            );
        }
    }
}
