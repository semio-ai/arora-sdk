//! A player: a playhead on a timeline, moved by a device clock.
//!
//! **The clock.** A player measures how far it has travelled from the
//! device time it last started or changed speed at, as
//! `round((now − anchor) × |speed|)` whole nanoseconds, computed exactly in
//! integers. Nothing is summed step by step, so a player stepped at 60 Hz and
//! one stepped at 1 kHz stand at the same place at the same device time,
//! however long they run.
//!
//! **The bounds.** A player plays within its window (by default its whole
//! length). One that starts outside the window, or whose window is changed
//! to one it is outside, plays over its whole length, `[0, length]`, until
//! it enters the window; from then on the window holds it. Reaching a bound,
//! the loop mode decides:
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
//! turn taken, the window not yet entered, not ended. `set_reversed` gives a
//! player the heading another has after a turn, so a player started late
//! plays alongside it.

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
    From { from: Nanos, travelled: i128 },
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
    /// The playhead stays where it is; outside the new window, it plays over
    /// the whole length until it enters it, as from a start outside.
    pub fn set_window(&mut self, at: Nanos, start: Nanos, end: Option<Nanos>, length: Nanos) {
        self.advance(at, length);
        self.window = (start, end);
        self.entered = false;
    }

    /// Heads the player against its speed's sign, as after a turn at a
    /// bound, or with it: the heading of another player that turned, for one
    /// that is to play alongside it. Reversed, it counts as having turned, so
    /// a bounce stops at the next bound.
    pub fn set_reversed(&mut self, at: Nanos, reversed: bool, length: Nanos) {
        self.advance(at, length);
        self.reversed = reversed;
        self.turned = self.turned || reversed;
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
    /// heading, applying the loop mode at every bound it reaches. The
    /// arithmetic is in `i128`, so no position, bound or distance a player
    /// can hold overflows it.
    fn travel(&mut self, mut distance: i128, length: Nanos) {
        let length = i128::from(length.max(0));
        let mut position = i128::from(self.position);
        while distance > 0 && !self.ended {
            let window = self.bounds_of_window(length as Nanos);
            let window = (i128::from(window.0), i128::from(window.1));
            if !self.entered && window.0 <= position && position <= window.1 {
                self.entered = true;
            }
            let (lo, hi) = if self.entered {
                window
            } else {
                (window.0.min(0), window.1.max(length))
            };
            let heading = i128::from(self.heading());
            if position < lo || position > hi {
                let toward = (position < lo) == (heading > 0);
                if toward {
                    let gap = if position < lo {
                        lo - position
                    } else {
                        position - hi
                    };
                    let step = distance.min(gap);
                    position += heading * step;
                    distance -= step;
                } else {
                    // Heading away from the bounds: the bound it left is
                    // where the rules take it from.
                    position = if heading > 0 { hi } else { lo };
                    distance = self.at_bound(heading, lo, hi, distance, &mut position);
                }
                continue;
            }
            if !self.entered {
                // The window ahead is entered on the way, at its near end.
                let gap = if heading > 0 && position < window.0 {
                    Some(window.0 - position)
                } else if heading < 0 && position > window.1 {
                    Some(position - window.1)
                } else {
                    None
                };
                if let Some(gap) = gap.filter(|gap| distance >= *gap) {
                    position += heading * gap;
                    distance -= gap;
                    self.entered = true;
                    continue;
                }
            }
            let room = if heading > 0 {
                hi - position
            } else {
                position - lo
            };
            if distance <= room {
                position += heading * distance;
                distance = 0;
                if position == if heading > 0 { hi } else { lo } && self.stops_at_bound() {
                    self.ended = true;
                }
            } else {
                position += heading * room;
                distance = self.at_bound(heading, lo, hi, distance - room, &mut position);
            }
        }
        self.position = position as Nanos;
    }

    /// Whether the loop mode stops the player at the next bound it reaches.
    fn stops_at_bound(&self) -> bool {
        match self.mode {
            LoopMode::Once => true,
            LoopMode::Bounce => self.turned,
            LoopMode::Loop | LoopMode::PingPong => false,
        }
    }

    /// The loop mode's rule at the bound `position` reached heading
    /// `heading`, with `remaining` nanoseconds still to travel; returns what
    /// is left to travel.
    fn at_bound(
        &mut self,
        heading: i128,
        lo: i128,
        hi: i128,
        remaining: i128,
        position: &mut i128,
    ) -> i128 {
        let span = hi - lo;
        if self.stops_at_bound() {
            self.ended = true;
            return 0;
        }
        if span == 0 {
            // A window of no length holds the playhead.
            return 0;
        }
        // Whole periods travel to where they started, so only the part of
        // the travel past them is left; a travel landing on a bound stays on
        // it. Before the window is entered they would not: the playhead
        // enters it on the way, within one period.
        let periods = |period: i128| {
            if self.entered {
                (remaining - 1) % period + 1
            } else {
                remaining
            }
        };
        match self.mode {
            LoopMode::Loop => {
                *position = if heading > 0 { lo } else { hi };
                periods(span)
            }
            LoopMode::Bounce | LoopMode::PingPong => {
                let left = if self.mode == LoopMode::PingPong {
                    periods(2 * span)
                } else {
                    remaining
                };
                self.reversed = !self.reversed;
                self.turned = true;
                left
            }
            LoopMode::Once => unreachable!("stops at its bound"),
        }
    }
}

/// How far a player travels in `elapsed` nanoseconds of device time at
/// `speed`: whole nanoseconds of timeline, the exact product rounded once
/// (half away from zero). It is computed in integers, so it neither loses a
/// nanosecond nor saturates however long the player runs; only a speed of
/// 2^63 or more saturates it, at a distance no window can tell from its
/// periods.
fn travel(elapsed: Nanos, speed: f64) -> i128 {
    if elapsed <= 0 || speed == 0.0 || !speed.is_finite() {
        return 0;
    }
    // |speed| = mantissa × 2^exponent, the mantissa an integer below 2^53.
    let bits = speed.abs().to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1 << 52) - 1);
    let (mantissa, exponent) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), biased - 1075)
    };
    // Below 2^63 × 2^53.
    let product = i128::from(elapsed) * i128::from(mantissa);
    if exponent >= 0 {
        return if exponent <= 10 {
            product << exponent
        } else {
            i128::MAX / 4
        };
    }
    let shift = -exponent;
    if shift > 117 {
        // The product is below 2^116: less than half a nanosecond.
        return 0;
    }
    (product + (1 << (shift - 1))) >> shift
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
    fn a_window_changed_while_playing_is_entered_on_the_way() {
        let mut p = playing(LoopMode::Loop);
        assert_eq!(at(&mut p, 4_000), 4_000);
        p.set_window(4_000 * MS, 1_000 * MS, Some(2_000 * MS), 5_000 * MS);
        // Outside the new window: on over the whole length, then into it.
        assert_eq!(at(&mut p, 4_100), 4_100);
        assert_eq!(at(&mut p, 6_500), 1_500);
        assert_eq!(at(&mut p, 7_200), 1_200);
        // Inside the new window, the window holds it at once.
        p.set_window(7_200 * MS, 0, Some(3_000 * MS), 5_000 * MS);
        assert_eq!(at(&mut p, 9_000), 3_000);
        assert_eq!(at(&mut p, 9_500), 500);
    }

    #[test]
    fn whole_periods_before_the_window_is_entered_do_not_skip_it() {
        let start = |p: &mut Player| {
            p.set_mode(LoopMode::Loop);
            p.set_window(0, 1_000 * MS, Some(3_000 * MS), 5_000 * MS);
            p.seek(0, 4_000 * MS, 5_000 * MS);
            p.play(0, 5_000 * MS);
        };
        let (mut once, mut twice) = (Player::new(), Player::new());
        start(&mut once);
        start(&mut twice);
        // 1 s to the end, a wrap, 1 s into the window, then 5 s in it.
        assert_eq!(at(&mut once, 7_000), 2_000);
        at(&mut twice, 2_000);
        assert_eq!(at(&mut twice, 7_000), 2_000);
    }

    #[test]
    fn the_clock_is_exact_however_long_and_fast_a_player_runs() {
        let period: i128 = 1_000_000_000;
        // The position a looping player over [0, 1 s] has after travelling
        // `travel` nanoseconds from 0: a travel landing on the end stays.
        let expected = |travel: i128| -> Nanos {
            if travel > 0 && travel % period == 0 {
                period as Nanos
            } else {
                (travel % period) as Nanos
            }
        };
        // Past 2^53 nanoseconds (104 days), at speed 1.
        let mut p = Player::new();
        p.set_mode(LoopMode::Loop);
        p.play(0, period as Nanos);
        let late = 9_007_199_254_740_993;
        p.advance(late, period as Nanos);
        assert_eq!(p.position(), expected(i128::from(late)));
        // Fast enough that a second of travel overflows i64.
        let speed = 3e9 + 0.25;
        let mut p = Player::new();
        p.set_mode(LoopMode::Loop);
        p.set_speed(0, speed, period as Nanos);
        p.play(0, period as Nanos);
        for elapsed in [1_000_000_000_i64, 1_000_000_001, 5_000_000_003] {
            p.advance(elapsed, period as Nanos);
            // The exact product, rounded half away from zero.
            let travel = (i128::from(elapsed) * 12_000_000_001 + 2) / 4;
            assert_eq!(p.position(), expected(travel), "at {elapsed}");
        }
    }

    #[test]
    fn a_turn_near_the_largest_length_does_not_overflow() {
        let length = 1 << 62;
        let mut p = Player::new();
        p.set_mode(LoopMode::PingPong);
        p.seek(0, length, length);
        p.play(0, length);
        p.advance(1, length);
        assert_eq!(p.position(), length - 1);
        assert!(p.reversed());
    }

    #[test]
    fn a_player_given_another_ones_heading_plays_alongside_it() {
        let mut leader = playing(LoopMode::PingPong);
        assert_eq!(at(&mut leader, 6_000), 4_000);
        assert!(leader.reversed());
        let mut late = Player::new();
        late.set_mode(LoopMode::PingPong);
        late.seek(6_000 * MS, 4_000 * MS, 5_000 * MS);
        late.play(6_000 * MS, 5_000 * MS);
        late.set_reversed(6_000 * MS, leader.reversed(), 5_000 * MS);
        for ms in [6_500, 10_000, 11_250, 17_000] {
            assert_eq!(at(&mut late, ms), at(&mut leader, ms), "at {ms} ms");
            assert_eq!(late.reversed(), leader.reversed());
        }
        // A bounce given a turn's heading stops at the next bound.
        let mut bounce = Player::new();
        bounce.set_mode(LoopMode::Bounce);
        bounce.seek(0, 4_000 * MS, 5_000 * MS);
        bounce.play(0, 5_000 * MS);
        bounce.set_reversed(0, true, 5_000 * MS);
        assert_eq!(at(&mut bounce, 5_000), 0);
        assert!(bounce.ended());
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
