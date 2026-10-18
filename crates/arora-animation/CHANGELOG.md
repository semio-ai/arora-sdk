# Changelog

All notable changes to `arora-animation`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - unreleased

### Added

- Clip format 2.0 (`clip`): tracks of keypoints at signed nanoseconds of
  clip-local time, each side of a keypoint the transition its author chose
  (an explicit handle in time and value units, an easing, or an inferred
  tangent), with a JSON document form; clip format 1.x read into it
  (`clip1`).
- Exact evaluation (`curve`): value and velocity at a time or over a batch
  of times, each number segment's control points placed by Semio Studio's
  editor rules (`resolve`).
- Players on a device clock (`player`): a window, `once`, `loop`, `bounce`
  and `ping_pong`, a signed speed, a start at a given device time, in integer
  nanoseconds measured from the last start, so a player does not drift.
- The engine (`engine`): clips, players and their instances (an offset and a
  speed multiplier each), stepped together into one value per key, blended
  by weight; bakes.
