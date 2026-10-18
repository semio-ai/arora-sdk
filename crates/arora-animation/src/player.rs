//! A player: a playhead on a timeline, moved by a device clock.
//!
//! **The clock.** A player measures how far it has travelled from the
//! device time it last started or changed speed at, as
//! `round((now − anchor) × |speed|)` whole nanoseconds. Nothing is summed
//! step by step, so a player stepped at 60 Hz and one stepped at 1 kHz stand
//! at the same place at the same device time, however long they run.
//!
//! **The bounds.** A player plays within its window (by default its whole
//! length). One that starts outside the window plays over its whole length,
//! `[0, length]`, until it enters the window; from then on the window holds
//! it. Reaching a bound, the loop mode decides:
//!
//! | Mode | At a bound |
//! |---|---|
//! | [`LoopMode::Once`] | stops on it: the player has ended |
//! | [`LoopMode::Loop`] | wraps to the other bound |
//! | [`LoopMode::Bounce`] | turns back the first time, then stops at the other bound |
//! | [`LoopMode::PingPong`] | turns back every time |
//!
//! These are Semio Studio's transport rules. Where Studio drops the part of a
//! frame's travel that passes a bound, a player carries it on past the wrap
//! or the turn, so its position at a device time does not depend on how the
//! device steps.
//!
//! **Direction.** The speed is signed: a negative speed plays backwards. A
//! turn at a bound reverses the heading without changing the speed; a new
//! speed of the other sign, `play` and `seek` start the rules afresh — no
//! turn taken, the window not yet entered, not ended.

use crate::time::Nanos;

/// What a player does at the bounds of its window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopMode {
    Once,
    Loop,
    Bounce,
    PingPong,
}

impl LoopMode {
    /// The names the animation module's `set_loop` takes.
    pub fn name(self) -> &'static str {
        match self {
            LoopMode::Once => "once",
            LoopMode::Loop => "loop",
            LoopMode::Bounce => "bounce",
            LoopMode::PingPong => "ping_pong",
        }
    }

    pub fn named(name: &str) -> Option<LoopMode> {
        [
            LoopMode::Once,
            LoopMode::Loop,
            LoopMode::Bounce,
            LoopMode::PingPong,
        ]
        .into_iter()
        .find(|m| m.name() == name)
    }
}

/// Whether a player's playhead advances.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Playing,
    Paused,
    Stopped,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Playing => "playing",
            State::Paused => "paused",
            State::Stopped => "stopped",
        }
    }
}

/// Where a playing player's travel is measured from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Clock {
    /// Not playing.
    Idle,
    /// Playing, and travelling from the device time `from`: `travelled`
    /// nanoseconds of timeline so far.
    From { from: Nanos, travelled: Nanos },
    /// Playing from the device time `at`, which is still to come.
    Waiting { at: Nanos },
}

/// A playhead and its transport.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    position: Nanos,
    state: State,
    speed: f64,
    mode: LoopMode,
    window: (Nanos, Option<Nanos>),
    /// Heading against the speed's sign, after a turn at a bound.
    reversed: bool,
    /// Turned at a bound since the rules last started afresh.
    turned: bool,
    /// Inside its window since the rules last started afresh.
    entered: bool,
    /// Stopped at a bound by the loop mode.
    ended: bool,
    clock: Clock,
    /// The device time the player was last moved to.
    now: Option<Nanos>,
}

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}

impl Player {
    /// A player at 0, paused, at speed 1, playing once over its whole
    /// length.
    pub fn new() -> Player {
        Player {
            position: 0,
            state: State::Paused,
            speed: 1.0,
            mode: LoopMode::Once,
            window: (0, None),
            reversed: false,
            turned: false,
            entered: false,
            ended: false,
            clock: Clock::Idle,
            now: None,
        }
    }

    pub fn position(&self) -> Nanos {
        self.position
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// The speed as it was set: the multiplier of device time, negative
    /// backwards.
    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// The speed the playhead moves at now, signed: the set speed, reversed
    /// after a turn at a bound; 0 while it does not move.
    pub fn velocity(&self) -> f64 {
        if !self.is_moving() {
            return 0.0;
        }
        if self.reversed {
            -self.speed
        } else {
            self.speed
        }
    }

    pub fn mode(&self) -> LoopMode {
        self.mode
    }

    /// The window as set: its start, and its end, `None` to end at the
    /// player's length.
    pub fn window(&self) -> (Nanos, Option<Nanos>) {
        self.window
    }

    /// Whether the loop mode stopped the player at a bound. It holds there
    /// until it is played, sought, or given a speed of the other sign.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Whether the player heads against its speed's sign: it turned back at
    /// a bound.
    pub fn reversed(&self) -> bool {
        self.reversed
    }

    /// Whether the player is waiting for the device time it was told to
    /// start at.
    pub fn waiting(&self) -> bool {
        matches!(self.clock, Clock::Waiting { .. })
    }

    /// Whether the playhead advances with the clock: playing, started, and
    /// not ended.
    pub fn is_moving(&self) -> bool {
        self.state == State::Playing && !self.ended && !self.waiting()
    }

    /// Starts playing from where the player is, at device time `at`.
    pub fn play(&mut self, at: Nanos, length: Nanos) {
        self.advance(at, length);
        self.state = State::Playing;
        self.afresh();
        self.clock = Clock::From {
            from: at,
            travelled: 0,
        };
    }

    /// Starts playing at device time `start`: until then the player holds,
    /// and from then on it stands where it would had it started at that
    /// instant, however its steps fall around it. `at` is when the command
    /// is given.
    pub fn play_at(&mut self, at: Nanos, start: Nanos, length: Nanos) {
        self.advance(at, length);
        self.state = State::Playing;
        self.afresh();
        self.clock = Clock::Waiting { at: start };
    }

    /// Holds the playhead where it is at device time `at`.
    pub fn pause(&mut self, at: Nanos, length: Nanos) {
        self.advance(at, length);
        self.state = State::Paused;
        self.clock = Clock::Idle;
    }

    /// Holds the playhead at its window's start.
    pub fn stop(&mut self, at: Nanos, length: Nanos) {
        self.advance(at, length);
        self.state = State::Stopped;
        self.clock = Clock::Idle;
        self.position = self.bounds_of_window(length).0;
        self.afresh();
    }

    /// Moves the playhead to `position` at device time `at`; a playing
    /// player plays on from there.
    pub fn seek(&mut self, at: Nanos, position: Nanos, length: Nanos) {
        self.advance(at, length);
        self.position = position;
        self.afresh();
        self.restart_clock(at);
    }

    /// Sets the speed from device time `at`. A speed of the other sign is a
    /// new direction: the rules start afresh.
    pub fn set_speed(&mut self, at: Nanos, speed: f64, length: Nanos) {
        self.advance(at, length);
        if (speed < 0.0) != (self.speed < 0.0) {
            self.afresh();
        }
        self.speed = speed;
        self.restart_clock(at);
    }

    pub fn set_mode(&mut self, mode: LoopMode) {
        self.mode = mode;
    }

    /// Sets the window: from `start` to `end`, or to the player's length.
    /// The playhead stays where it is; outside the window, it plays toward
    /// it.
    pub fn set_window(&mut self, at: Nanos, start: Nanos, end: Option<Nanos>, length: Nanos) {
        self.advance(at, length);
        self.window = (start, end);
    }

    /// The window as it bounds a player of `length`, its ends in order.
    pub fn bounds_of_window(&self, length: Nanos) -> (Nanos, Nanos) {
        let (start, end) = (self.window.0, self.window.1.unwrap_or(length));
        (start.min(end), start.max(end))
    }

    /// Moves the playhead with the clock to device time `now`, under the
    /// bounds of a player `length` long. A time before the last one the
    /// player was moved to moves nothing.
    pub fn advance(&mut self, now: Nanos, length: Nanos) {
        if self.now.is_some_and(|last| now < last) {
            return;
        }
        self.now = Some(now);
        if self.state != State::Playing {
            return;
        }
        if let Clock::Waiting { at } = self.clock {
            if now < at {
                return;
            }
            self.clock = Clock::From {
                from: at,
                travelled: 0,
            };
        }
        let Clock::From { from, travelled } = self.clock else {
            return;
        };
        let total = travel(now.saturating_sub(from), self.speed);
        self.clock = Clock::From {
            from,
            travelled: total,
        };
        self.travel(total - travelled, length);
    }

    /// The rules start afresh: no turn taken, the window not entered, not
    /// ended, heading with the speed.
    fn afresh(&mut self) {
        self.reversed = false;
        self.turned = false;
        self.entered = false;
        self.ended = false;
    }

    fn restart_clock(&mut self, at: Nanos) {
        if let Clock::From { .. } = self.clock {
            self.clock = Clock::From {
                from: at,
                travelled: 0,
            };
        }
    }

    fn heading(&self) -> i64 {
        let forward = self.speed >= 0.0;
        if forward != self.reversed {
            1
        } else {
            -1
        }
    }

    /// Moves the playhead `distance` nanoseconds of timeline along its
    /// heading, applying the loop mode at every bound it reaches.
    fn travel(&mut self, mut distance: Nanos, length: Nanos) {
        let length = length.max(0);
        while distance > 0 && !self.ended {
            let window = self.bounds_of_window(length);
            if !self.entered && window.0 <= self.position && self.position <= window.1 {
                self.entered = true;
            }
            let (lo, hi) = if self.entered {
                window
            } else {
                (0.min(window.0), length.max(window.1))
            };
            let heading = self.heading();
            if self.position < lo || self.position > hi {
                let toward = (self.position < lo) == (heading > 0);
                if toward {
                    let gap = if self.position < lo {
                        lo - self.position
                    } else {
                        self.position - hi
                    };
                    let step = distance.min(gap);
                    self.position += heading * step;
                    distance -= step;
                } else {
                    // Heading away from the bounds: the bound it left is
                    // where the rules take it from.
                    self.position = if heading > 0 { hi } else { lo };
                    distance = self.at_bound(heading, lo, hi, distance);
                }
                continue;
            }
            if !self.entered {
                // The window ahead is entered on the way, at its near end.
                let gap = if heading > 0 && self.position < window.0 {
                    Some(window.0 - self.position)
                } else if heading < 0 && self.position > window.1 {
                    Some(self.position - window.1)
                } else {
                    None
                };
                if let Some(gap) = gap.filter(|gap| distance >= *gap) {
                    self.position += heading * gap;
                    distance -= gap;
                    self.entered = true;
                    continue;
                }
            }
            let room = if heading > 0 {
                hi - self.position
            } else {
                self.position - lo
            };
            if distance <= room {
                self.position += heading * distance;
                distance = 0;
                if self.position == if heading > 0 { hi } else { lo } && self.stops_at_bound() {
                    self.ended = true;
                }
            } else {
                self.position += heading * room;
                distance = self.at_bound(heading, lo, hi, distance - room);
            }
        }
    }

    /// Whether the loop mode stops the player at the next bound it reaches.
    fn stops_at_bound(&self) -> bool {
        match self.mode {
            LoopMode::Once => true,
            LoopMode::Bounce => self.turned,
            LoopMode::Loop | LoopMode::PingPong => false,
        }
    }

    /// The loop mode's rule at the bound the playhead reached heading
    /// `heading`, with `remaining` nanoseconds still to travel; returns what
    /// is left to travel.
    fn at_bound(&mut self, heading: i64, lo: Nanos, hi: Nanos, remaining: Nanos) -> Nanos {
        let span = hi - lo;
        if self.stops_at_bound() {
            self.ended = true;
            return 0;
        }
        if span == 0 {
            // A window of no length holds the playhead.
            return 0;
        }
        match self.mode {
            LoopMode::Loop => {
                self.position = if heading > 0 { lo } else { hi };
                // Whole periods travel to where they started; a travel
                // landing on a bound stays on it.
                (remaining - 1) % span + 1
            }
            LoopMode::Bounce | LoopMode::PingPong => {
                self.reversed = !self.reversed;
                self.turned = true;
                if self.mode == LoopMode::PingPong {
                    (remaining - 1) % (2 * span) + 1
                } else {
                    remaining
                }
            }
            LoopMode::Once => unreachable!("stops at its bound"),
        }
    }
}

/// How far a player travels in `elapsed` nanoseconds of device time at
/// `speed`: whole nanoseconds of timeline, rounded once from the start.
fn travel(elapsed: Nanos, speed: f64) -> Nanos {
    if elapsed <= 0 {
        return 0;
    }
    let distance = (elapsed as f64 * speed.abs()).round();
    if distance >= i64::MAX as f64 {
        i64::MAX
    } else {
        distance as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Nanos = 1_000_000;

    /// A player playing over `[0, 5 s]` at speed 1 from device time 0.
    fn playing(mode: LoopMode) -> Player {
        let mut p = Player::new();
        p.set_mode(mode);
        p.play(0, 5_000 * MS);
        p
    }

    fn at(p: &mut Player, ms: i64) -> Nanos {
        p.advance(ms * MS, 5_000 * MS);
        p.position() / MS
    }

    #[test]
    fn a_once_player_stops_at_the_end_on_the_step_that_reaches_it() {
        let mut p = playing(LoopMode::Once);
        assert_eq!(at(&mut p, 4_990), 4_990);
        assert!(!p.ended());
        assert_eq!(at(&mut p, 5_000), 5_000);
        assert!(p.ended());
        assert_eq!(at(&mut p, 9_000), 5_000);
        assert!(!p.is_moving());
    }

    #[test]
    fn a_loop_wraps_carrying_its_overshoot() {
        let mut p = playing(LoopMode::Loop);
        assert_eq!(at(&mut p, 5_000), 5_000);
        assert_eq!(at(&mut p, 5_016), 16);
        assert_eq!(at(&mut p, 23_000), 3_000);
    }

    #[test]
    fn a_bounce_turns_once_and_stops_at_the_start() {
        let mut p = playing(LoopMode::Bounce);
        assert_eq!(at(&mut p, 6_000), 4_000);
        assert!(p.reversed());
        assert_eq!(at(&mut p, 9_000), 1_000);
        assert_eq!(at(&mut p, 11_000), 0);
        assert!(p.ended());
    }

    #[test]
    fn a_ping_pong_turns_at_every_bound() {
        let mut p = playing(LoopMode::PingPong);
        assert_eq!(at(&mut p, 12_000), 2_000);
        assert!(!p.reversed());
        assert_eq!(at(&mut p, 17_000), 3_000);
        assert!(p.reversed());
    }

    #[test]
    fn a_player_outside_its_window_plays_the_whole_length_until_it_enters() {
        let mut p = Player::new();
        p.set_mode(LoopMode::Loop);
        p.set_window(0, 1_000 * MS, Some(2_000 * MS), 5_000 * MS);
        p.seek(0, 4_000 * MS, 5_000 * MS);
        p.play(0, 5_000 * MS);
        // Wraps over [0, 5 s] first, enters the window at 1 s, then loops in it.
        assert_eq!(at(&mut p, 1_500), 500);
        assert_eq!(at(&mut p, 2_500), 1_500);
        assert_eq!(at(&mut p, 3_600), 1_600);
    }

    #[test]
    fn a_speed_scales_the_travel_and_its_sign_plays_backwards() {
        let mut p = playing(LoopMode::Once);
        p.set_speed(1_000 * MS, 0.5, 5_000 * MS);
        assert_eq!(at(&mut p, 3_000), 2_000);
        p.set_speed(3_000 * MS, -2.0, 5_000 * MS);
        assert_eq!(at(&mut p, 3_500), 1_000);
        assert_eq!(at(&mut p, 4_500), 0);
        assert!(p.ended());
    }

    #[test]
    fn play_at_holds_until_its_instant_then_stands_where_it_would_have() {
        let mut p = Player::new();
        p.play_at(0, 1_000 * MS + 3, 5_000 * MS);
        p.advance(990 * MS, 5_000 * MS);
        assert_eq!(p.position(), 0);
        assert!(p.waiting());
        p.advance(1_010 * MS, 5_000 * MS);
        assert_eq!(p.position(), 10 * MS - 3);
    }

    #[test]
    fn the_position_at_a_device_time_does_not_depend_on_the_steps() {
        for mode in [LoopMode::Loop, LoopMode::PingPong, LoopMode::Bounce] {
            let length = 1_234_567_891;
            let mut fast = Player::new();
            let mut slow = Player::new();
            for p in [&mut fast, &mut slow] {
                p.set_mode(mode);
                p.set_window(0, 100_000_000, Some(1_100_000_017), length);
                p.set_speed(0, 1.75, length);
                p.play(0, length);
            }
            let hour = 3_600 * 1_000 * MS;
            let mut t = 0;
            while t < hour {
                t += 1_000_000; // 1 kHz
                fast.advance(t, length);
                if t % 16_666_667 == 0 || t % 7 == 3 {
                    slow.advance(t, length);
                }
            }
            slow.advance(hour, length);
            assert_eq!(fast, slow, "{mode:?}");
        }
    }
}
