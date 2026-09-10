# Changelog

All notable changes to `arora-bridge-ros2`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [6.3.0] - 2026-09-10

### Added

- **The ROS4HRI preset publishes the rendered face.** Two outbound endpoints,
  one per `image_transport` transport, on the names PAL OS documents under
  `/robot_face/image_raw/*`: the key `display/face` as a `sensor_msgs/Image`
  on `/robot_face/image_raw`, and `display/face/compressed` as a
  `sensor_msgs/CompressedImage` on `/robot_face/image_raw/compressed`. A face
  writes the key of the transport it encodes and the other never publishes;
  `coverage()` reports that key as unserved, which is what a consumer of that
  transport would find. A device exposed through the preset no longer declares
  the image itself with `with_typed_output_on`.
## [6.2.1] - 2026-09-10

### Fixed

- **The crate requires `arora-msgs-ros2` 1.2.** 6.2.0 declared `1`, which a
  consumer's older lockfile satisfies with 1.0.0 — a version without the
  hashers the bridge calls, and without the `communication_skills/Say` types
  its ROS4HRI preset binds — so the bridge failed to compile there. The
  requirement now names the version its code needs.

## [6.2.0] - 2026-09-10

### Added

- **The ROS4HRI preset serves the speech skill**: `/skill/say`
  (`communication_skills/Say`) bound to a device's `say` method, the goal's
  `input` carrying the utterance. What the run feeds back rides
  `std_skills/Feedback` — for a face, the viseme at the audio playhead, so a
  ROS client watching the goal sees the lipsync stream.
- **A record feeds a bound action's feedback field by field, by name.** A run
  that reports several things at once — a face's `{viseme, intensity}` — lands
  on the message without knowing its ids; an entry the message has no field
  for is dropped, and a record landing nothing is no feedback.

### Changed

- **A bound action may leave a method parameter unrouted**, and it then runs on
  the method's own default. A standard contract carries what the standard says,
  which need not be every parameter the implementation takes — `Say` has no
  field for a voice. The gap is logged when the binding resolves. A route
  naming a field the goal lacks, or a parameter the method lacks, is still
  refused: a typo is not an omission.

## [6.1.0] - 2026-09-09

### Fixed

- **Bound actions and raw services reach native `rmw_zenoh` clients.** A
  service is keyed like a topic — `<domain>/<name>/<type>/<type_hash>` — and a
  native client addresses a server by the hash its generated type computes;
  the bridge's servers announced the placeholder, so `/skill/look_at` and
  `/skill/say` were listed, described, and never reached (the request went to
  a key nobody served). A bound action's `_SendGoal`, `_GetResult` and
  `_FeedbackMessage` are now keyed on the hashes computed from its registry
  goal, result and feedback (`arora_msgs_ros2::action_hashes`, matching
  `rosidl` byte for byte), and a synthesized service on
  `arora_msgs_ros2::service_rihs01` of its request and response
  (`ros2-client-multi-rmw` 0.13.0's `_with_type_hash` constructors). A
  synthesized action keeps the placeholder: its result and feedback are typed
  from what the run writes, unknown when the server is made. A type the hasher
  cannot describe keeps the placeholder and says so once. DDS is unaffected.
- **Typed publishers reach native `rmw_zenoh` subscribers.** `rmw_zenoh` keys
  every publisher on its message's REP-2016 type hash, and a native subscriber
  listens on that exact key. A typed key publisher announced the all-zero
  placeholder — the value `ros2-client` uses for a type its hash table does not
  know — so a `sensor_msgs/CompressedImage` (or any typed output beyond the
  `std_msgs` scalars) was listed by `ros2 topic list`, described correctly by
  `ros2 topic info -v`, and never received by `ros2 topic echo`, `rqt_image_view`
  or anything else on the Zenoh RMW. The bridge holds each message's full type
  description in its registry, which is all the hash needs, so it now computes
  the hash there and keys the publisher on it (`ros2-client-multi-rmw` 0.12.1's
  `create_raw_publisher_with_type_hash`). A type the hasher cannot describe
  still publishes, reaching only other `ros2-client` peers, and says so once.
  Inbound was never affected: subscriptions match on a wildcard. DDS is
  unaffected either way — its discovery carries no such hash.

## [6.0.0] - 2026-09-07

### Fixed

- **The outbound seam no longer queues without bound.** `try_send` fed a `tokio`
  *unbounded* channel drained by one awaited publish per changed key, so a
  publish path slower than the device's step rate grew it for as long as that
  lasted. It is now a **latest-value-per-key** buffer: what crosses this seam is
  state, not events, and a key written again before the node task drains
  replaces its pending value instead of queuing behind it. Memory is bounded by
  the device's key count however far behind the ROS graph falls, what is
  published is always the freshest value, and samples can no longer be emitted
  out of order under load. The first time a value is superseded before
  publication — the moment the ROS graph falls behind the device — is reported
  once at `debug`.
- **The bridge no longer publishes on topics it subscribes to.** An input key
  was republished from the store on its own topic and re-ingested as a new
  inbound update, so every input key the device ever received circulated through
  the ROS graph indefinitely — converging on a single writer, but reviving stale
  values whenever a real command and the echo crossed (a released control
  re-asserting itself seconds later). Neither middleware filters this for us:
  DDS delivers a participant's own writes to its own readers, and the Zenoh
  backend declares subscribers with no origin filter. Keys whose outbound topic
  is one of the bridge's own subscriptions are now skipped, with a warning
  naming the key.

### Added

- **Per-endpoint delivery profiles** (`Qos`), defaulting by flow: state flowing
  out is `Qos::SensorData` (best-effort, volatile, keep-last-1 — ROS's
  sensor-data profile), commands flowing in are `Qos::Reliable`. Set the `qos`
  field on `Endpoint`, `Include`, `InputKey`, `TypedInput` or `TypedOutput` to
  override. Both backends map it onto their own profile type.
- A warning, once per key, when a value serializes to more than 64 KB of JSON on
  the scalar plane — a topic carrying an image-sized sample is a configuration
  mistake, and it is expensive in a way that is otherwise invisible (see below).

### Changed — BREAKING

- `Endpoint`, `Include`, `InputKey`, `TypedInput` and `TypedOutput` each gained
  a `qos: Option<Qos>` field, so literal construction must name it.
- Outbound key topics are **best-effort** by default, where they were reliable.
  A best-effort writer does not match a reliable reader: a subscriber on the
  rclcpp/rclpy default sees nothing on the data plane until it asks for
  best-effort (`ros2 topic echo --qos-reliability best_effort`). Declare
  `Qos::Reliable` on the endpoint to keep the old behaviour.
- `conversions::setup_key_subscriber`, `setup_typed_key_subscriber`,
  `setup_typed_key_publisher` and `KeyPublisher::create` take a `Qos`, and
  `KeyPublisher::publish` returns the size of the JSON it published (0 on every
  other arm).

### Known issue — not fixed here

Publishing **large samples over the DDS backend retains memory per sample**,
inside RustDDS rather than in this crate. Measured with a vizij face publishing
its rendered frame on the scalar plane (a ~70 KB PNG becomes ~300 KB of JSON):
the process grew ~120 MB in twenty seconds and never gave it back. Bisected by
holding the code path fixed and varying only the payload — a 200-byte sample at
the same rate on the same publisher costs nothing, and encoding the full sample
without handing it to the writer costs nothing either. RustDDS fragments
anything over its 1 KB `data_max_size_serialized`, so image-sized values are the
shape that triggers it.

Neither the latest-value buffer nor the QoS default addresses this: the samples
had already reached a publisher. Until it is understood upstream, keep
image-sized values off DDS topics — hence the size warning above.

## [5.0.0] - 2026-07-31

### Added

- **The skill plane**: an `ExposureProfile` carries `ActionBinding`s — a
  standard ROS 2 action (a registry `.action` type on an absolute name such as
  `/skill/look_at`) bound to a device task-run method, checked at startup
  against `DescribeMethods` and refused loudly when the contract does not hold.
  Bound actions ride the existing goal-lifecycle machinery with registry
  message types instead of synthesised ones, serve one goal at a time under
  `std_skills/Meta.priority` (equal-or-higher replaces, reporting `ROS_EINTR`;
  lower is rejected), and answer the standard Result message with the
  `std_skills` errno of the goal's lifecycle — unless the run wrote the Result,
  or an errno, itself. Scalar feedback lands on the matching
  `std_skills/Feedback` field.
- The `ros4hri` preset binds `interaction_skills/LookAt` on `/skill/look_at` to
  a `(policy, target, frame)` `look_at` method, the goal's point coerced to the
  store's vec3 form. `coverage()` reports the skill plane alongside the others.

### Changed — BREAKING

- `ExposureProfile` and `Ros2BridgeConfig` gained public fields, and
  `coverage()` gained a `functions` parameter.

## [4.0.0] - 2026-07-31

### Changed — BREAKING

- Re-pinned to `arora-msgs-ros2` 1.0.0, whose constant-quote fix changes
  generated constant values. Those types are part of this crate's public
  surface, so the bump travels with it (`arora-hal-ros2` 3.0.0 in the same
  lockstep).

## [3.5.0] - 2026-07-31

### Added

- **Exposure profiles**: one profile bundles the whole surface a deployment
  exposes. Typed `Endpoint`s bind absolute topics to registered message types
  and fan their fields out over device keys by dotted name (a structured
  message maps to several keys — no string rewrite could express that), and
  glob `Include`s (`*` one segment, `**` the rest; regex was rejected) rewrite
  bulk keys' prefixes onto absolute names on the scalar plane.
  `ExposureProfile::coverage` reports what a device does not serve, so a face
  is checked against its profile up front rather than topic by topic.
- `ExposureProfile::ros4hri()`: the ROS4HRI face surface for both incumbent
  name sets — PAL (`/robot_face/*`) and IIIA (`/expressive_face/*`).
  Expression commands fan out to `standard/ros4hri/expression/*`, `look_at`
  points land as the gaze target (an `x`/`y`/`z` structure coerces to the
  store's vec3 form) and frame, speech text feeds the lipsync key. Enabling the
  profile is the only wiring a face device needs.
- Field routes resolve by name against the message's runtime type and land
  atomically: all routed fields of one message arrive in one `StateChange`.

## [3.4.0] - 2026-07-30

### Added

- Typed input topics (ARORA-85 inbound): a device key can subscribe to a
  registered ROS message rather than a `std_msgs` scalar —
  `Ros2BridgeConfig::with_typed_input(path, ros_type)` (and
  `with_typed_input_on(path, ros_type, topic)` for an absolute topic name). The
  bridge subscribes raw (ros2-client `create_raw_subscription`, 0.12) and
  decodes each message against its runtime type through the shared CDR codec
  into a single-key change — so a native ROS4HRI publisher of e.g.
  `hri_msgs/Expression` lands the device key. Completes the typed topic plane
  (outbound shipped in 3.3.0).

### Changed

- ros2-client re-pinned to 0.12 (adds `RawSubscription`).

## [3.3.0] - 2026-07-30

### Added

- Typed output topics (ARORA-85): a device key can be declared to publish as a
  registered ROS message rather than a `std_msgs` scalar —
  `Ros2BridgeConfig::with_typed_output(path, ros_type)` (and
  `with_typed_output_on(path, ros_type, topic)` for an absolute topic name).
  The key's value is encoded against the message's runtime type through the
  shared CDR codec and written to a raw publisher, so a key can ride a typed
  ROS4HRI message such as `hri_msgs/Expression`. A key not declared typed still
  publishes on the untyped `std_msgs` path. Outbound + DDS; inbound-typed and
  native-zenoh interop are follow-ups (they need `create_raw_subscription` and
  a type-hash raw publisher in ros2-client).

## [3.2.0] - 2026-07-29

### Added

- **The action plane** (`/{namespace}/actions/{name}`): every *task-run* method
  — one returning the behavior-tree `Status` enumeration, the signature of a
  spawnable behavior — is exposed as a full ROS 2 action, discovered like the
  service plane (nothing declared). SendGoal spawns the run through the
  engine's interpreter-module SPAWN ABI (spoken over the value plane, pinned
  against `arora-behavior` by a dev-dependency test); the goal advances by
  watching the run's per-goal status key on the outbound state stream;
  CancelGoal issues the handle's stop call (the interpreter's halt) and the
  halt-ended run reports `CANCELED`; GetResult resolves at terminal with the
  result value the run wrote, lazily typed — as is feedback. Message types are
  synthesised at runtime and served over ros2-client's raw action server
  (`ros2-client-multi-rmw` 0.11.0), on both backends; the status topic is
  transient-local with history 1, per the ROS actions design. Actions claim
  exactly the methods the service plane skips (enum returns), so the planes
  never overlap. Covered by synthesis/lifecycle unit tests and a live-DDS
  LookAt lifecycle test (goal → feedback → cancel → errno result) against a
  typed ROS 2 action client.

### Changed

- `ros2-client-multi-rmw` 0.10.2 → 0.11.0 (the raw action server / raw
  publisher).

## [3.1.0] - 2026-07-22

### Added

- Dual middleware backend. A `zenoh` feature selects an rmw_zenoh-compatible
  ROS 2-over-Zenoh backend alongside the default `dds` backend; exactly one is
  active. Consumers typically expose these as `ros2-dds` / `ros2-zenoh`.

### Changed

- `ros2-client` now comes from the `ros2-client-multi-rmw` fork (still imported
  as `ros2_client`), which carries both backends. Native `std_msgs` scalar
  topics interoperate with C++ `rmw_zenoh` peers on the send direction. The
  default (`dds`) build is unchanged for existing consumers.

## [3.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 / `arora-bridge` 4 (their types are part of this
  API).

## [2.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [2.0.0] - 2026-07-09

### Breaking

- Receiver-as-stream endpoints
- Delete the io pump; step drives the sync bridge/HAL seams

## [1.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

## [0.1.0] - 2026-07-07

### Added

- ROS 2 as an Arora Bridge (the ros2 bridge seam)

