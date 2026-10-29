# Changelog

All notable changes to `arora-bridge-ros2`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [11.2.0] - 2026-10-29

### Added

- The bridge asks the device for its inputs and functions again every two
  seconds, and exposes what the device gained since: an input it opens, or a
  key its HAL describes, gets its topic, a function a module it loads brings
  gets its service or action, and a profile binding whose function appears is
  served. What the device lost — an input it closed, a module it unloaded —
  loses its endpoint, and a function reloaded with another signature gets its
  endpoint anew. A key the bridge publishes stops being published once its
  topic becomes an input's. The asks go through the device's in-process
  command channel, and an answer equal to the last one changes nothing.

## [11.1.0] - 2026-10-21

### Changed

- Requires `ros2-client-multi-rmw` 0.14 (RustDDS 0.14) and arora-msgs-ros2
  2.4. On DDS a reliable writer's send window is now
  `ResourceLimits::max_samples` (8192 when unset) rather than its history
  depth: a reliable key topic's writer no longer blocks for its 100 ms, then
  drops the sample, each time a reader is behind by one, and the feedback,
  status and result writers of an action hold back a message only once 8192
  samples await a reader that stopped acknowledging. RustDDS 0.14 also
  reassembles DATAFRAG under reliable delivery, matches XCDR1/XCDR2 data
  representations and handles stale ACKNACKs.

## [11.0.2] - 2026-10-19

### Fixed

- The interpreter module's functions — `spawn`, `spawn_graph` and `halt`, and
  the task runs the interpreter implements — are left out of service
  resolution and out of the start-up warning that names the methods with no
  ROS 2 form. A task run the interpreter implements still becomes an action.

## [11.0.1] - 2026-10-19

### Fixed

- The device's refusal of a write from an input topic is logged, with the
  topic, the keys and the device's reason (a key it did not open, a value of
  another type than the key states). One topic's refusals for the same keys are
  logged at most once every 5 seconds, those in between counted into the next
  line, so a topic publishing at 100 Hz does not flood the log.

## [11.0.0] - 2026-10-13

### Added

- **Routes take each element of a sequence, and durations as seconds.** A
  `*` segment collects what the rest of the path reaches in every element
  into one flat array (`timeline.visemes.*.viseme.value` is a `[u8]`), and a
  `builtin_interfaces/Duration` routes as its seconds, an `f32`. A bound
  action's goal route accepts a numeric index, as an inbound endpoint's does.
- **Skills on topics.** A `TopicSkillBinding` in an `ExposureProfile` serves
  a device task-run method on a topic: each message spawns the method, its
  fields routed onto the parameters as an action goal's are, and nothing
  answers. The latest message wins: the run the previous message spawned is
  halted, unless it already ended, before the new one is spawned, and
  messages queued while a spawn is in flight collapse to the newest. A
  binding is checked at startup against the device's described methods and
  refused loudly when its contract does not hold.
- The ROS4HRI preset serves Vizij's face skills, typed by arora-msgs-ros2
  2.3's `vizij_face_msgs`: `set_facial_action_units(codes: [u8], intensities:
  [f32])` on the topic `/skill/set_facial_action_units`, `set_viseme(viseme:
  u8)` on the topic `/skill/set_viseme`, and `play_visemes(visemes: [u8],
  starts: [f32], durations: [f32])` on the action `/skill/play_visemes`.
- `ExposureProfile::with_set_expression_on_keys` lands `/skill/set_expression`
  on the `standard/ros4hri/expression/*` keys.

### Changed

- **Breaking:** `ExposureProfile` has the `topic_skills` field and
  `Ros2BridgeConfig` the `topic_skill_bindings` field, so a struct literal of
  either must name it. `ExposureProfile::ros4hri()` and
  `Ros2BridgeConfig::new` build them as before.
- **Breaking:** the ROS4HRI preset serves `/skill/set_expression` as a topic
  skill: a message spawns the device's `set_expression(expression: string,
  valence: f32, arousal: f32)` task run instead of landing on the
  `standard/ros4hri/expression/*` keys. A face that reads those keys takes
  `ExposureProfile::ros4hri().with_set_expression_on_keys()`.
- Depends on arora-msgs-ros2 2.3.

## [10.4.2] - 2026-10-13

### Fixed

- A reader of an action's status or `GetResult` replies that stops
  acknowledging no longer holds up the action. On DDS such a reader keeps
  that writer's send window full, and each status or reply write blocked
  the action task for up to 100 ms, delaying every goal it serves; a reply
  whose write timed out was lost. The status and result writers now have
  no blocking time, like the feedback writer. A full window keeps the
  newest status array, and every refused reply until it is written. What
  a window holds back is retried with a backoff of 10 ms doubling to 1 s;
  feedback now starts its backoff at 10 ms too. A window is reported full
  once it has stayed full for a second, not each time a reader is slow to
  acknowledge.

## [10.4.1] - 2026-10-13

### Fixed

- A reader of an action's feedback that stops acknowledging no longer
  holds back the goal's status and result. On DDS such a reader keeps the
  feedback writer's send window full. Each feedback publish blocked until
  its write timed out, and the state changes behind it queued without
  bound, so the result came seconds behind the run. Feedback is now a
  latest-value stream. It is published without waiting on any reader: the
  DDS feedback writer has no blocking time, and a full window keeps only
  the newest feedback per goal and feedback key. That feedback is retried
  with a backoff of 100 ms doubling to 1 s, and dropped once its goal
  finishes.

## [10.4.0] - 2026-10-12

### Added

- The ROS4HRI preset takes closed captions: an
  `hri_actions_msgs/ClosedCaption` on `/closed_captions` lands its `text` on
  `standard/ros4hri/caption/text` and its `speaker_id` on
  `standard/ros4hri/caption/speaker`, in one change.
- The ROS4HRI preset publishes the device's diagnostics: the key
  `diagnostics` holds a whole `diagnostic_msgs/DiagnosticArray`, published
  on `/diagnostics`. It is the preset's one reliable outbound endpoint,
  because the ROS diagnostics tooling subscribes reliably and a best-effort
  writer matches no reliable reader.

### Fixed

- A message holding a sequence or fixed array of messages — a
  `DiagnosticArray`'s `status` — has a default to compose on: an empty
  sequence, or that many default messages. A typed output of such a type
  was refused at its first key write, and a bound action's result or
  feedback of such a type could not be built.

## [10.3.1] - 2026-10-12

### Fixed

- A message on an input's topic that does not decode as the topic's
  `std_msgs` type is dropped, and the input stays subscribed. RustDDS matches
  a writer of another type on the same topic name (an `Int32` on a `Float64`
  input) to the bridge's reader; its messages counted as receive failures,
  and five in a row ended the input's subscription for the rest of the run.
  The first dropped message after a decoded one is logged at warn, the rest
  at debug.

## [10.3.0] - 2026-10-12

### Changed

- An input whose shape has no `std_msgs` scalar, and that a typed endpoint
  routes onto, gets no JSON `std_msgs/String` key topic: the endpoint is its
  interface, and the bridge says so once at info. With the ROS4HRI preset, the
  `u8` viseme code `standard/ros4hri/viseme` comes in on `/tts/viseme`
  (`hri_msgs/Viseme`) and `/tts/visemes` (`hri_msgs/Visemes`) only. An input of
  such a shape no endpoint serves keeps the JSON fallback.

### Fixed

- Every key reaches ROS 2 under a valid name. A key path is escaped token by
  token — a character outside `[A-Za-z0-9_]` becomes `_`, a run of
  underscores collapses to one, a token starting with a digit gains a leading
  `_`, an empty token becomes `_` — so a dotted key such as
  `rig/poses/pose_b.weight` or a UUID-named rig key now has a topic, inbound
  and outbound, where it had none. A path that is already a valid name keeps
  its exact topic. An include's rewrite escapes the key the same way.
- Of two keys escaping to one topic, the key that sorts first keeps it; the
  other gets no topic and is reported once, naming both. An output never
  publishes on another key's input topic.
- A key whose publisher cannot be created is reported once and no longer
  retried, instead of warning on every change.

## [10.2.0] - 2026-10-10

### Added

- The ROS4HRI preset publishes the lip shape being spoken as a
  `std_msgs/String` on `/robot_face/viseme`, from the speech state key
  `standard/ros4hri/speech/viseme`: one of the Vizij standard's shape names,
  `sil` at rest. Anything that follows the lips reads one topic for the whole
  robot, whichever player is speaking.

## [10.1.1] - 2026-10-10

### Fixed

- Requires `ros2-client-multi-rmw` 0.13.1. Before it, a peer's
  `wait_for_subscription` on the bridge's topics, and `wait_for_service` on its
  services, could miss a match under load and wait forever: the match event was
  dropped from a bounded status channel after the wait's one-time check
  (semio-ai/ros2-client#42).

## [10.1.0] - 2026-10-10

### Added

- The ROS4HRI preset lands a streamed viseme's `time` (seconds into the
  utterance) and `duration` (seconds the shape is held) on
  `standard/ros4hri/viseme/time` and `standard/ros4hri/viseme/duration`, in
  the same change as its code on `standard/ros4hri/viseme`, from either
  `/tts/viseme` or `/tts/visemes`. A viseme player can then hold each shape
  as long as the speech does, and tell a repeated shape from the one before.

## [10.0.0] - 2026-10-06

### Removed

- **Breaking:** the ROS4HRI preset no longer serves IIIA's
  `/expressive_face/look_at`; a gaze point comes in on PAL's
  `/robot_face/look_at` or through the `/skill/look_at` action. The preset
  serves PAL's `/robot_face/*` names only.

## [9.0.0] - 2026-10-06

### Changed

- **Breaking:** the ROS4HRI preset takes expression commands as the
  interaction skill's `interaction_skills/SetExpression` on
  `/skill/set_expression`, and no longer subscribes to `/robot_face/expression`.
  The `hri_msgs/Expression` the message carries fans out to the same
  `standard/ros4hri/expression/*` keys as before.

## [8.1.0] - 2026-10-02

### Added

- **Methods with optional parameters or returns are served.** An optional of a
  scalar or a message travels as the bounded sequence `T[<=1]` in a synthesized
  service's request and reply and in a synthesized action's goal: empty is
  absent, one element is present, and a message holding more is refused. A
  run's result or feedback that is an optional of a scalar is typed the same
  way. An optional array still has no ROS 2 form, and its method is skipped.

### Changed

- Depends on arora-msgs-ros2 2.1.

## [8.0.1] - 2026-09-29

### Fixed

- **What an unrouted goal parameter becomes.** A parameter an action binding
  does not route is left out of the spawn call. The resolve-time log said the
  method then ran "on its own default"; it now states that an absent optional
  parameter is `None`, and that the implementation supplies an absent
  required one or fails the run. `ActionBinding::goal_routes` no longer says
  every parameter must be routed.

## [8.0.0] - 2026-09-29

### Changed

- **Breaking:** the input topics are the device's inputs — each key its store
  opens to remote writers (`KeyMeta::editable`), typed from its meta — asked of
  the device at startup. `Ros2BridgeConfig::with_input`, its `inputs` field and
  `InputKey` are gone: an input is stated once, in the store, and every bridge
  exposes the same set. An input whose meta states no shape gets no topic, and
  is reported.
- **Breaking:** depends on arora-bridge 6, whose `BridgeOp::ListKeys` replies
  with each key's `KeyMeta`.

## [7.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3, arora-bridge 5, arora-msgs-ros2 2
  and arora-behavior 9.
- A method with an optional parameter or return is skipped, like any other
  signature ROS 2 cannot carry.

## [6.5.0] - 2026-09-24

### Added

- **Sequence indexing in a field route.** A numeric segment of an inbound
  route's dotted path indexes an array (`visemes.0.value`), so a message that
  wraps its payload in a sequence routes by the same mechanism as any other —
  no message type is special-cased in the subscriber.
- **The ROS4HRI preset takes a streamed viseme**, landing its ROS4HRI code on
  `standard/ros4hri/viseme` from either shape a TTS node publishes it in: one
  `hri_msgs/Viseme` on `/tts/viseme`, or an `hri_msgs/Visemes` on
  `/tts/visemes`. Both carry the shape at the audio playhead, so a sequence's
  first viseme is the one that lands; a message holding a whole alignment is a
  timeline, and playing one out over time is a viseme player's work rather
  than a bridge's. This is how a face lipsyncs to speech synthesized
  elsewhere — a `/skill/say` run drives the lips from the run itself.

## [6.4.0] - 2026-09-21

### Added

- **Outbound field fan-in.** A typed output composes its message from several
  device keys, the mirror of the inbound fan-out: each `FieldRoute` places one
  key's value at a dotted field of the message (a scalar coerced to the
  field's kind, a vec3 key becoming a point, a nested message or an array
  taken as is), the other fields keep their last value (their type's default
  until written), and a change to any routed key publishes the whole message
  again — once per change, however many of its keys the change carried. An
  `ExposureProfile`'s outbound endpoints route every field they declare
  instead of only their first route's key; `TypedOutput` carries the routes.
- **The ROS4HRI preset publishes what the face is saying**: `/robot_face/speech`
  (`std_msgs/String`) from the speech state key
  `standard/ros4hri/speech/text` — the utterance while a say run speaks, empty
  at rest — for subtitles and transcripts.

### Removed

- **The preset's text inputs `/robot_face/tts` and `/expressive_face/speech`.**
  Text is not commanded through a topic: speaking is the `/skill/say` action,
  which is what produces visemes and the speech state. The key they landed on
  is now the speech state key above, written by the device.

## [6.3.1] - 2026-09-17

### Fixed

- **Action and service servers match native DDS clients.** Every service
  endpoint the bridge serves — the plain method services and an action's goal,
  result, cancel and feedback — rides volatile durability on both the request
  and the reply side, the profile `rmw_qos_profile_services_default` gives
  every rclcpp, rclpy and `ros2` CLI client. They requested transient-local,
  and DDS refuses to match a reader that requests more durability than the
  writer offers, so under `rmw_fastrtps_cpp` a native action client waited on
  "action server not available" forever while `ros2 action list` showed the
  server. The status topic stays transient-local. A `ros2-client` peer must
  request volatile on its services to match, as a native client does — the
  crate's tests do, and a consumer's own test clients need the same. The Zenoh
  backend, which matches without durability, is unaffected.

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

