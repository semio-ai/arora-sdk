//! Gestures as behavior-tree leaves: timed joint targets for a NAOqi robot's keys.
//!
//! A gesture is an episodic leaf, like `Say`: `Running` while it plays, `Success` once it
//! has played and for as long as it keeps being ticked, so it pairs with a sentence in a
//! `Parallel`. Its clock is the tree's (`dt_ns`, bound to `arora/dt`); a gesture whose
//! leaf goes [`IDLE_RESTART`] unticked was halted, and plays from its start when ticked
//! again.
//!
//! Each joint a gesture moves is two out-parameters, bound to the joint's
//! `target_position` and `target_stiffness` keys: NAOqi moves no joint whose stiffness is
//! zero, so a gesture stiffens what it moves, and relaxes it once over.

// A gesture takes one parameter per joint target and stiffness, and the module macro
// generates a call wrapper with the same arguments.
#![allow(clippy::too_many_arguments)]

use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use arora_behavior::Status;

/// How long a gesture's leaf goes unticked before the gesture is considered halted.
pub const IDLE_RESTART: Duration = Duration::from_millis(250);

/// How long `hands_ready` plays.
pub const HANDS_READY_DURATION: Duration = Duration::from_secs(4);

/// The stiffness a gesture holds the joints it moves with: enough to move a wrist and a
/// hand, soft enough to be safe to touch.
pub const GESTURE_STIFFNESS: f64 = 0.6;

/// How far the wrists rotate either side of their neutral, in radians (about 20°).
const WRIST_AMPLITUDE: f64 = 0.35;
/// One wrist rotation back and forth, in seconds.
const WRIST_PERIOD: f64 = 2.0;
/// One hand opening and closing, in seconds.
const HAND_PERIOD: f64 = 4.0 / 3.0;

#[arora_module::module(
    id = "840bf9e0-5f8f-4ada-a809-ab6c97375d69",
    name = "naoqi-gestures",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "Gestures of NAOqi robots as behavior-tree leaves: timed joint targets"
)]
pub mod naoqi_gestures {
    use super::*;

    /// Hands that say "ready", over [`HANDS_READY_DURATION`]: both wrists rotate slightly
    /// back and forth while both hands open and close three times. Targets and stiffness
    /// are written every tick while it plays; once over, the wrists are back at their
    /// neutral, the hands closed, and the four joints relaxed.
    #[export(id = "8a6a1ae1-a446-42b2-9053-55f252d1a580")]
    pub fn hands_ready(
        #[param(id = "65143ea3-abac-464b-9678-9df66abc81db")] dt_ns: u64,
        #[param(id = "e0a4313c-417e-4e1e-9ecf-2beaa5d96a67")] l_wrist_yaw: &mut f64,
        #[param(id = "4b8607d6-5060-4e79-9489-be32767650dd")] r_wrist_yaw: &mut f64,
        #[param(id = "b181886c-de5f-4827-9707-c41c0b54e123")] l_hand: &mut f64,
        #[param(id = "938ec18f-4a1d-4ba7-94d3-a3482a4edd16")] r_hand: &mut f64,
        #[param(id = "8c3aa47e-d426-4883-84cf-fc156e68084e")] l_wrist_yaw_stiffness: &mut f64,
        #[param(id = "a3c7a3d8-f951-4e82-996a-fdfa5e7d4048")] r_wrist_yaw_stiffness: &mut f64,
        #[param(id = "b0d6c54a-8dde-46c9-b6c6-e29c9bec2555")] l_hand_stiffness: &mut f64,
        #[param(id = "eecc2c27-357d-4595-854e-224b0c6930bc")] r_hand_stiffness: &mut f64,
    ) -> Status {
        let elapsed = lock(&HANDS_READY).advance(dt_ns, Instant::now());
        let pose = hands_ready_pose(elapsed);
        *l_wrist_yaw = -pose.wrist_yaw;
        *r_wrist_yaw = pose.wrist_yaw;
        *l_hand = pose.hand;
        *r_hand = pose.hand;
        *l_wrist_yaw_stiffness = pose.stiffness;
        *r_wrist_yaw_stiffness = pose.stiffness;
        *l_hand_stiffness = pose.stiffness;
        *r_hand_stiffness = pose.stiffness;
        if elapsed < HANDS_READY_DURATION {
            Status::Running
        } else {
            Status::Success
        }
    }
}

/// Where `hands_ready` is at some time from its start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandsPose {
    /// The right wrist's yaw, in radians; the left one mirrors it.
    pub wrist_yaw: f64,
    /// Both hands' opening, from 0 (closed) to 1 (open).
    pub hand: f64,
    /// The four joints' stiffness.
    pub stiffness: f64,
}

/// The pose of `hands_ready` at `elapsed` from its start.
pub fn hands_ready_pose(elapsed: Duration) -> HandsPose {
    if elapsed >= HANDS_READY_DURATION {
        return HandsPose {
            wrist_yaw: 0.0,
            hand: 0.0,
            stiffness: 0.0,
        };
    }
    let t = elapsed.as_secs_f64();
    let tau = std::f64::consts::TAU;
    HandsPose {
        wrist_yaw: WRIST_AMPLITUDE * (tau * t / WRIST_PERIOD).sin(),
        hand: 0.5 - 0.5 * (tau * t / HAND_PERIOD).cos(),
        stiffness: GESTURE_STIFFNESS,
    }
}

/// A gesture's clock: the tree's time since the gesture started, restarted after a halt.
#[derive(Default)]
struct GestureClock {
    elapsed: Duration,
    last_tick: Option<Instant>,
}

impl GestureClock {
    fn advance(&mut self, dt_ns: u64, now: Instant) -> Duration {
        match self.last_tick {
            Some(last) if now.duration_since(last) <= IDLE_RESTART => {
                self.elapsed += Duration::from_nanos(dt_ns);
            }
            // First tick, or ticked again after a halt: play from the start.
            _ => self.elapsed = Duration::ZERO,
        }
        self.last_tick = Some(now);
        self.elapsed
    }
}

static HANDS_READY: LazyLock<Mutex<GestureClock>> = LazyLock::new(Mutex::default);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_ready_starts_at_rest_moves_then_relaxes() {
        let start = hands_ready_pose(Duration::ZERO);
        assert_eq!(start.wrist_yaw, 0.0);
        assert_eq!(start.hand, 0.0);
        assert_eq!(start.stiffness, GESTURE_STIFFNESS);
        let quarter = hands_ready_pose(Duration::from_millis(500));
        assert!((quarter.wrist_yaw - WRIST_AMPLITUDE).abs() < 1e-9);
        let open = hands_ready_pose(Duration::from_secs_f64(HAND_PERIOD / 2.0));
        assert!((open.hand - 1.0).abs() < 1e-9);
        let over = hands_ready_pose(HANDS_READY_DURATION);
        assert_eq!(over.stiffness, 0.0);
        assert_eq!(over.wrist_yaw, 0.0);
    }

    #[test]
    fn the_clock_follows_the_tree_and_restarts_after_a_halt() {
        let mut clock = GestureClock::default();
        let now = Instant::now();
        assert_eq!(clock.advance(20_000_000, now), Duration::ZERO);
        let next = now + Duration::from_millis(20);
        assert_eq!(clock.advance(20_000_000, next), Duration::from_millis(20));
        let after_halt = next + IDLE_RESTART + Duration::from_millis(1);
        assert_eq!(clock.advance(20_000_000, after_halt), Duration::ZERO);
    }
}
