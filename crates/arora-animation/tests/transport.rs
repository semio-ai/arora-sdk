//! Studio's transport rules, as a player carries them out: the bounds a
//! player plays within, and what each loop mode does at them.
//!
//! These are the rules of Studio's `Transport.advance` (in semio_studio,
//! `apps/studio/src/arora/transport.ts`), with one difference throughout:
//! where Studio sets the playhead on the bound and drops the rest of a
//! frame's travel, a player carries the rest on past the turn or the wrap.
//! The cases below state where the playhead stands as a player places it.
//! What the UI does when the person presses play (jumping to the start from
//! the end) stays the UI's.

use arora_animation::player::State;
use arora_animation::{LoopMode, Nanos, Player};

const MS: Nanos = 1_000_000;
/// The timeline's length in these cases: Studio's default 5 s.
const LENGTH: Nanos = 5_000 * MS;

/// A player at `from_ms` in `mode`, playing at `speed` from device time 0.
fn player(mode: LoopMode, from_ms: i64, speed: f64) -> Player {
    let mut p = Player::new();
    p.set_mode(mode);
    p.set_speed(0, speed, LENGTH);
    p.seek(0, from_ms * MS, LENGTH);
    p.play(0, LENGTH);
    p
}

/// The playhead, in milliseconds, at device time `ms`.
fn at(p: &mut Player, ms: i64) -> i64 {
    p.advance(ms * MS, LENGTH);
    p.position() / MS
}

#[test]
fn bouncing_at_the_end_turns_the_player_back_and_keeps_its_speed() {
    let mut p = player(LoopMode::PingPong, 4_000, 1.0);
    assert_eq!(at(&mut p, 1_500), 4_500);
    assert!(p.reversed());
    assert_eq!(
        p.speed(),
        1.0,
        "the direction the person chose is unchanged"
    );
    assert_eq!(p.velocity(), -1.0);
}

#[test]
fn bouncing_at_the_start_turns_a_reversed_player_forward() {
    let mut p = player(LoopMode::PingPong, 1_000, -1.0);
    assert_eq!(at(&mut p, 1_500), 500);
    assert!(p.reversed());
    assert_eq!(p.velocity(), 1.0);
}

#[test]
fn a_bounce_without_looping_turns_once_and_then_stops() {
    let mut p = player(LoopMode::Bounce, 4_000, 1.0);
    assert_eq!(at(&mut p, 1_500), 4_500);
    assert!(p.reversed() && !p.ended());
    assert_eq!(at(&mut p, 6_000), 0);
    assert!(p.ended());
    assert!(p.reversed(), "it stops heading the way it turned");
    assert_eq!(at(&mut p, 9_000), 0);
}

#[test]
fn a_bounce_with_looping_turns_at_every_bound() {
    let mut p = player(LoopMode::PingPong, 4_000, 1.0);
    assert_eq!(at(&mut p, 3_000), 3_000);
    assert!(p.reversed());
    assert_eq!(at(&mut p, 6_500), 500);
    assert!(!p.reversed(), "turned again at the start");
    assert_eq!(at(&mut p, 60_000), 4_000);
    assert!(!p.ended());
}

#[test]
fn looping_wraps_to_the_start_and_reversed_to_the_end() {
    let mut p = player(LoopMode::Loop, 4_000, 1.0);
    assert_eq!(at(&mut p, 1_500), 500);
    let mut p = player(LoopMode::Loop, 1_000, -1.0);
    assert_eq!(at(&mut p, 1_500), 4_500);
    assert!(!p.reversed(), "a loop does not turn");
}

#[test]
fn neither_looping_nor_bouncing_stops_at_the_bound() {
    let mut p = player(LoopMode::Once, 4_000, 1.0);
    assert_eq!(at(&mut p, 1_500), 5_000);
    assert!(p.ended());
    assert_eq!(p.state(), State::Playing, "playing, but held");
    assert!(!p.is_moving());
    let mut p = player(LoopMode::Once, 1_000, -1.0);
    assert_eq!(at(&mut p, 1_500), 0);
    assert!(p.ended());
}

#[test]
fn custom_bounds_are_where_a_player_inside_them_turns() {
    let mut p = player(LoopMode::PingPong, 3_000, 1.0);
    p.set_window(0, 1_000 * MS, Some(4_000 * MS), LENGTH);
    assert_eq!(at(&mut p, 1_250), 3_750);
    assert!(p.reversed());
}

#[test]
fn a_player_before_its_bounds_plays_toward_them_and_then_keeps_to_them() {
    let mut p = player(LoopMode::PingPong, 1_000, 1.0);
    p.set_window(0, 2_000 * MS, Some(3_000 * MS), LENGTH);
    // Toward the bounds over the whole timeline, then within them.
    assert_eq!(at(&mut p, 500), 1_500);
    assert_eq!(at(&mut p, 1_500), 2_500);
    assert_eq!(at(&mut p, 2_500), 2_500);
    assert!(p.reversed());
    assert_eq!(
        at(&mut p, 3_200),
        2_200,
        "turned at the start bound, not at 0"
    );
    assert!(!p.reversed());
    assert_eq!(at(&mut p, 3_600), 2_600);
}

#[test]
fn a_player_after_its_bounds_plays_back_toward_them() {
    let mut p = player(LoopMode::Loop, 4_500, -1.0);
    p.set_window(0, 1_500 * MS, Some(3_500 * MS), LENGTH);
    assert_eq!(at(&mut p, 2_000), 2_500);
    assert_eq!(at(&mut p, 3_500), 3_000, "wrapped from 1.5 s to 3.5 s");
}

#[test]
fn a_step_that_crosses_into_the_window_keeps_to_it_from_where_it_entered() {
    // One step carries the player from after its bounds, into them, and on
    // past their start: it wraps at the bound it entered within.
    let mut p = player(LoopMode::Loop, 4_500, -1.0);
    p.set_window(0, 1_500 * MS, Some(3_500 * MS), LENGTH);
    assert_eq!(at(&mut p, 3_500), 3_000);
}

#[test]
fn a_player_heading_away_from_its_bounds_plays_over_the_whole_timeline() {
    // Studio's full range: [0, length] bounds a player that has not entered
    // its bounds, so it wraps there and comes round to them.
    let mut p = player(LoopMode::Loop, 4_000, 1.0);
    p.set_window(0, 1_000 * MS, Some(2_000 * MS), LENGTH);
    assert_eq!(at(&mut p, 1_500), 500);
    assert_eq!(at(&mut p, 2_000), 1_000);
    assert_eq!(at(&mut p, 3_500), 1_500, "looping within [1 s, 2 s] now");
    // Stopping, a player heading away from its bounds stops at the
    // timeline's end.
    let mut p = player(LoopMode::Once, 4_000, 1.0);
    p.set_window(0, 1_000 * MS, Some(2_000 * MS), LENGTH);
    assert_eq!(at(&mut p, 2_000), 5_000);
    assert!(p.ended());
}

#[test]
fn a_new_direction_play_and_seek_start_the_rules_afresh() {
    let mut p = player(LoopMode::Bounce, 4_000, 1.0);
    assert_eq!(at(&mut p, 1_500), 4_500);
    assert!(p.reversed());
    // A speed of the same sign keeps the turn; of the other sign, a new run.
    p.set_speed(1_500 * MS, 2.0, LENGTH);
    assert!(p.reversed());
    p.set_speed(1_500 * MS, -1.0, LENGTH);
    assert!(!p.reversed());
    assert_eq!(at(&mut p, 2_500), 3_500);
    // An ended player plays again from where it stopped.
    let mut p = player(LoopMode::Once, 4_000, 1.0);
    assert_eq!(at(&mut p, 2_000), 5_000);
    p.seek(2_000 * MS, 0, LENGTH);
    assert!(!p.ended());
    assert_eq!(at(&mut p, 2_100), 100);
    let mut p = player(LoopMode::Bounce, 4_000, 1.0);
    assert_eq!(at(&mut p, 1_500), 4_500);
    p.play(1_500 * MS, LENGTH);
    assert!(!p.reversed(), "play restarts the bounce");
}

#[test]
fn pausing_holds_and_playing_resumes_from_the_same_place() {
    let mut p = player(LoopMode::Loop, 0, 1.0);
    assert_eq!(at(&mut p, 1_000), 1_000);
    p.pause(1_000 * MS, LENGTH);
    assert_eq!(at(&mut p, 9_000), 1_000);
    p.play(9_000 * MS, LENGTH);
    assert_eq!(at(&mut p, 9_250), 1_250);
    p.stop(9_250 * MS, LENGTH);
    assert_eq!(p.position(), 0, "stop returns to the window's start");
}

/// A player at `from_ms` in `mode`, held (paused) at device time 0.
fn held(mode: LoopMode, from_ms: i64) -> Player {
    let mut p = Player::new();
    p.set_mode(mode);
    p.seek(0, from_ms * MS, LENGTH);
    p.pause(0, LENGTH);
    p
}

#[test]
fn a_held_player_keeps_the_heading_it_is_given_for_its_start() {
    // Started by `play_at`: it heads back from its first step.
    let mut p = held(LoopMode::Bounce, 3_000);
    p.set_reversed(0, true, LENGTH);
    p.play_at(0, 0, LENGTH);
    assert!(p.reversed());
    assert_eq!(at(&mut p, 500), 2_500);
    // Started by `play`.
    let mut p = held(LoopMode::Bounce, 3_000);
    p.set_reversed(0, true, LENGTH);
    p.play(0, LENGTH);
    assert_eq!(at(&mut p, 500), 2_500);
    // Through a `seek` before the start.
    let mut p = held(LoopMode::Bounce, 3_000);
    p.set_reversed(0, true, LENGTH);
    p.seek(0, 2_000 * MS, LENGTH);
    assert!(p.reversed(), "a seek keeps a held player's heading");
    p.play(0, LENGTH);
    assert_eq!(at(&mut p, 500), 1_500);
    // The heading counts as a turn taken: a bounce ends at the next bound.
    assert_eq!(at(&mut p, 2_000), 0);
    assert!(p.ended());
}

#[test]
fn a_kept_heading_serves_one_start_and_survives_the_commands_before_it() {
    // Kept for one start: the next is afresh.
    let mut p = held(LoopMode::PingPong, 3_000);
    p.set_reversed(0, true, LENGTH);
    p.play(0, LENGTH);
    assert!(p.reversed());
    p.pause(0, LENGTH);
    p.play(0, LENGTH);
    assert!(!p.reversed(), "a second start heads with the speed");
    // Kept on a stopped player, and through a speed of the same sign.
    let mut p = held(LoopMode::PingPong, 3_000);
    p.stop(0, LENGTH);
    p.seek(0, 3_000 * MS, LENGTH);
    p.set_reversed(0, true, LENGTH);
    p.set_speed(0, 2.0, LENGTH);
    p.play_at(0, 0, LENGTH);
    assert_eq!(at(&mut p, 500), 2_000);
}

#[test]
fn a_player_not_reversed_starts_forward() {
    let mut p = held(LoopMode::Bounce, 3_000);
    p.play_at(0, 0, LENGTH);
    assert!(!p.reversed());
    assert_eq!(at(&mut p, 500), 3_500);
    // A heading given back to the speed's sign is kept too, with no turn
    // taken: the bounce turns at the end and has not ended.
    let mut p = held(LoopMode::Bounce, 3_000);
    p.set_reversed(0, true, LENGTH);
    p.set_reversed(0, false, LENGTH);
    p.play(0, LENGTH);
    assert_eq!(at(&mut p, 500), 3_500);
    assert_eq!(at(&mut p, 2_500), 4_500);
    assert!(p.reversed() && !p.ended());
}

#[test]
fn a_start_after_a_turn_or_a_reset_heads_with_the_speed() {
    // A turn taken while playing is not kept past a pause: a start is afresh.
    let mut p = player(LoopMode::PingPong, 4_000, 1.0);
    assert_eq!(at(&mut p, 1_500), 4_500);
    assert!(p.reversed());
    p.pause(1_500 * MS, LENGTH);
    p.play(1_500 * MS, LENGTH);
    assert!(!p.reversed());
    // Nor a heading given while playing.
    let mut p = player(LoopMode::PingPong, 1_000, 1.0);
    p.set_reversed(0, true, LENGTH);
    p.pause(0, LENGTH);
    p.play(0, LENGTH);
    assert!(!p.reversed());
    // A stop, or a speed of the other sign, leaves no heading kept.
    let mut p = held(LoopMode::PingPong, 3_000);
    p.set_reversed(0, true, LENGTH);
    p.stop(0, LENGTH);
    p.play(0, LENGTH);
    assert!(!p.reversed());
    let mut p = held(LoopMode::PingPong, 3_000);
    p.set_reversed(0, true, LENGTH);
    p.set_speed(0, -1.0, LENGTH);
    p.play(0, LENGTH);
    assert!(!p.reversed());
    assert_eq!(at(&mut p, 500), 2_500, "backwards by its speed alone");
}

/// The commands a client sends a held player to start it alongside one that
/// turned: a speed, the heading, then `play_at` the instant the others start
/// at. A device that takes each a round trip after the last — a real one —
/// stands exactly where one that took them in the same step does, from the
/// first step after its start onwards, turns included.
#[test]
fn a_heading_given_before_a_late_start_catches_up_the_way_it_heads() {
    const TRIP: Nanos = 37 * MS + 123;
    let start: Nanos = 100 * MS;
    let mut sibling = held(LoopMode::PingPong, 1_000);
    sibling.set_speed(start, 1.5, LENGTH);
    sibling.set_reversed(start, true, LENGTH);
    sibling.play_at(start, start, LENGTH);
    let mut late = held(LoopMode::PingPong, 1_000);
    late.set_speed(start + TRIP, 1.5, LENGTH);
    late.set_reversed(start + 2 * TRIP, true, LENGTH);
    late.play_at(start + 3 * TRIP, start, LENGTH);
    let mut now = start + 3 * TRIP;
    late.advance(now, LENGTH);
    assert!(
        late.reversed() && late.position() < 1_000 * MS,
        "it caught up heading back"
    );
    while now < start + 2_000 * MS {
        sibling.advance(now, LENGTH);
        late.advance(now, LENGTH);
        assert_eq!(late.position(), sibling.position(), "at {now} ns");
        assert_eq!(late.reversed(), sibling.reversed(), "at {now} ns");
        now += 16_666_667;
    }
    // It turned at the start bound and heads forward again.
    assert!(!late.reversed() && late.position() > 1_000 * MS);
}
