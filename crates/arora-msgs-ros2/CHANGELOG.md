# Changelog

All notable changes.

## [1.1.0] - 2026-09-10

### Added

- **REP-2016 hashes for services and actions.** `service_rihs01` hashes a
  service from its request and response types, and `action_hashes` an action's
  `_SendGoal`, `_GetResult` and `_FeedbackMessage` from its goal, result and
  feedback — reproducing what `rosidl` generates, wrappers included
  (`unique_identifier_msgs/UUID`, `builtin_interfaces/Time`,
  `service_msgs/ServiceEventInfo`, the `_Event` messages). These are the keys
  a native `rmw_zenoh` client addresses a server by.

## [0.1.1] - 2026-07-30

### Changed

- ros2-client re-pinned to 0.12.
