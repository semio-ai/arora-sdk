# Changelog

All notable changes.

## [1.2.0] - 2026-09-10

### Added

- **`communication_skills/Say`**, ROS4HRI's speech skill, vendored from the
  standard: goal `meta` + `person_id` + `group_id` + `input`, with the
  `std_skills` result and feedback the other skills use. A device serving a
  `say` method can now be bound to `/skill/say`. Its feedback carries Vizij's
  extension to the standard — `string viseme`, `float32 intensity` — after the
  standard's own field; the README's "Departures from upstream" table is the
  record of it.

## [1.1.0] - 2026-09-10

### Added

- **REP-2016 hashes for services and actions.** `service_rihs01` hashes a
  service from its request and response types, and `action_hashes` an action's
  `_SendGoal`, `_GetResult` and `_FeedbackMessage` from its goal, result and
  feedback — reproducing what `rosidl` generates, wrappers included
  (`unique_identifier_msgs/UUID`, `builtin_interfaces/Time`,
  `service_msgs/ServiceEventInfo`, the `_Event` messages). These are the keys
  a native `rmw_zenoh` client addresses a server by.

## [1.0.0] - 2026-07-31

### Breaking

- **A quoted string constant carries its value, not its quotes.**
  `string GLANCE="glance"` generated a constant holding `"glance"` with the
  quotes; per the ROS 2 interface rules quotes delimit and are not part of the
  value. Generated constant values change, so this is a major;
  `arora-bridge-ros2` 4.0.0 and `arora-hal-ros2` 3.0.0 re-pin in lockstep.

### Added

- **The ROS4HRI skill packages**, vendored from the standard's interface
  packages: `std_skills` (`Meta` with its priority protocol, `Result` with the
  errno vocabulary, `Feedback`) and `interaction_skills` (`SetExpression`,
  `LookAt.action`).
- **`.action` codegen.** The three `---`-separated sections of an action file
  become the REP-2016 sub-messages `<pkg>/action/<Name>_Goal`, `_Result` and
  `_Feedback` — the SendGoal/GetResult wrappers are wire conventions the action
  machinery composes, not definitions — and the registry indexes the `/action/`
  namespace like `/msg/`.

## [0.1.1] - 2026-07-30

### Changed

- ros2-client re-pinned to 0.12.

## [0.1.0] - 2026-07-29

First release: the ROS 2 message types an Arora device speaks. The
type-directed CDR codec (`cdr`, shared by the bridge and the HAL), the
`Ros2Registry` mapping every type by id and by name — the REP-2016
`geometry_msgs/msg/Point` and the short `geometry_msgs/Point` forms, types
defined at runtime accepted — `ros2_representable`, the bundled
`builtin_interfaces`, `geometry_msgs`, `hri_msgs`, `naoqi_bridge_msgs`,
`sensor_msgs`, `std_msgs` and `trajectory_msgs` packages generated from their
`.msg` files at build time, and the REP-2016 RIHS01 type hash behind the
`interop` feature.
