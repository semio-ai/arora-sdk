//! Exposure profiles: which device keys a bridge exposes to ROS 2, under
//! which absolute names, and with which message types (ARORA-86, Track A5 of
//! the ROS4HRI face plan).
//!
//! A profile has three planes, matching how ROS4HRI consumes a face:
//!
//! - [`Endpoint`]s bind an absolute topic to a registered message type and
//!   fan its fields out over device keys ([`FieldRoute`]) — the command
//!   surfaces, where a structured message maps to several keys and no string
//!   rewrite could express the relation — or, outbound, publish one device
//!   key that already holds the whole message (the rendered face).
//! - [`Include`]s select bulk data keys by **glob** (`*` one segment, `**`
//!   the rest — regex was rejected) and rewrite their prefix into an absolute
//!   topic name — the state plane, where key names and topic names correspond
//!   one-to-one.
//! - [`ActionBinding`]s bind an absolute ROS 2 **action** to a device task-run
//!   method — the skill plane, where long-running, cancellable work rides a
//!   standard `.action` contract (`interaction_skills/LookAt`) instead of a
//!   topic.
//! - [`TopicSkillBinding`]s bind an absolute **topic** to a device task-run
//!   method — the skill plane's other form, for a skill that sets a state
//!   held until changed (`set_expression`): each message spawns the method,
//!   fire-and-forget, and the latest message wins.
//!
//! [`ExposureProfile::coverage`] reports what a device does not serve, so a
//! deployment can check a face against the profile it enables instead of
//! discovering holes topic by topic.

use std::fmt::Write as _;

use crate::conversions::escape_path;
use crate::qos::Qos;

/// The direction of an exposed surface, from the device's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// ROS publishes, the device key receives (a command surface).
    In,
    /// The device key publishes to ROS (a state surface).
    Out,
}

/// One field of a typed endpoint routed to (or from) a device key.
#[derive(Debug, Clone)]
pub struct FieldRoute {
    /// Dotted path into the message (`"valence"`, `"header.frame_id"`,
    /// `"point"`). Empty routes the whole message.
    pub field: String,
    /// The device key the field lands on (or is read from).
    pub key: String,
}

/// An absolute topic bound to a registered message type, its fields fanned
/// out over device keys.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// Absolute ROS topic name, e.g. `/robot_face/look_at`.
    pub topic: String,
    /// Registered ROS message name, e.g. `hri_msgs/Expression`.
    pub ros_type: String,
    pub flow: Flow,
    /// Field fan-out. To route the whole message onto one key, declare one
    /// route with an empty `field`.
    pub routes: Vec<FieldRoute>,
    /// Delivery profile; `None` takes [`Qos::default_for`] this flow.
    pub qos: Option<Qos>,
}

/// A glob of device keys exposed on the scalar plane, with its prefix
/// rewritten into an absolute topic name.
#[derive(Debug, Clone)]
pub struct Include {
    /// Glob over device keys: `*` matches one path segment, a trailing `**`
    /// matches the rest.
    pub glob: String,
    /// The key prefix removed before publishing, e.g. `standard/ros4hri/`.
    pub strip: String,
    /// The topic prefix put in its place, e.g. `/robot_face/state/`.
    pub prefix: String,
    pub flow: Flow,
    /// Delivery profile for every key this include rewrites; `None` takes
    /// [`Qos::default_for`] this flow.
    pub qos: Option<Qos>,
}

/// A profile-declared ROS 2 action bound to a device task-run method: the
/// exterior contract of a **skill** — a standard `.action` type served on a
/// well-known name — systematically associated with the behavior that
/// implements it, the association checked against the device's described
/// methods at start and whenever they change.
#[derive(Debug, Clone)]
pub struct ActionBinding {
    /// Absolute ROS 2 action name, e.g. `/skill/look_at`.
    pub action: String,
    /// Registered ROS action type name, e.g. `interaction_skills/LookAt`; its
    /// `<Name>_Goal` / `<Name>_Result` / `<Name>_Feedback` messages must be in
    /// the registry (a vendored `.action`, or defined at runtime).
    pub ros_type: String,
    /// The device method a goal spawns as a task run. It must be
    /// action-shaped (return the behavior-tree `Status`) per the device's
    /// DescribeMethods answer.
    pub function: String,
    /// Goal fan-out onto the function's parameters: [`FieldRoute::field`] is
    /// the dotted path into the goal message, [`FieldRoute::key`] names the
    /// parameter it becomes. A parameter no route names is left out of the
    /// spawn call, and what an absent argument means is the implementation's:
    /// a method declared with arora-module reads an absent optional parameter
    /// as `None`, and fails the call on an absent required one.
    pub goal_routes: Vec<FieldRoute>,
}

/// A profile-declared topic bound to a device task-run method: the exterior
/// contract of a **skill on a topic** — a `set_*` skill, which sets a state
/// that holds until changed, so it has no lifecycle for a client to follow.
///
/// Every message on the topic spawns the method as a task run, its fields
/// routed onto the method's parameters as an [`ActionBinding`]'s goal is.
/// Nothing answers the publisher. The latest message wins: the run the
/// previous message spawned is halted, if it has not ended, before the new
/// one is spawned, and messages arriving while a spawn is in flight collapse
/// to the newest. The association is checked against the device's described
/// methods, like an action binding's.
#[derive(Debug, Clone)]
pub struct TopicSkillBinding {
    /// Absolute ROS 2 topic name, e.g. `/skill/set_expression`.
    pub topic: String,
    /// Registered ROS message name, e.g. `interaction_skills/SetExpression`.
    pub ros_type: String,
    /// The device method each message spawns as a task run. It must be
    /// action-shaped (return the behavior-tree `Status`) per the device's
    /// DescribeMethods answer.
    pub function: String,
    /// Message fan-out onto the function's parameters, as
    /// [`ActionBinding::goal_routes`]: [`FieldRoute::field`] is the path into
    /// the message, [`FieldRoute::key`] names the parameter.
    pub routes: Vec<FieldRoute>,
    /// Delivery profile; `None` takes [`Qos::default_for`] an inbound flow.
    pub qos: Option<Qos>,
}

/// A named set of [`Endpoint`]s, [`Include`]s, [`ActionBinding`]s and
/// [`TopicSkillBinding`]s — everything a deployment says with "expose this
/// face as `<profile>`".
#[derive(Debug, Clone)]
pub struct ExposureProfile {
    pub name: String,
    pub endpoints: Vec<Endpoint>,
    pub includes: Vec<Include>,
    pub actions: Vec<ActionBinding>,
    pub topic_skills: Vec<TopicSkillBinding>,
}

impl ExposureProfile {
    /// The ROS4HRI face surface, under PAL's `/robot_face/*` names, out of
    /// the box:
    ///
    /// - the `set_expression` skill (`interaction_skills/SetExpression` on
    ///   the topic `/skill/set_expression`) spawns the device's
    ///   `set_expression(expression: string, valence: f32, arousal: f32)`
    ///   task run, from the `hri_msgs/Expression` it carries (its
    ///   `confidence` is perception's and is not routed).
    ///   [`with_set_expression_on_keys`](Self::with_set_expression_on_keys)
    ///   lands it on the `standard/ros4hri/expression/*` keys instead;
    /// - Vizij's `set_facial_action_units` skill
    ///   (`vizij_face_msgs/SetFacialActionUnits` on the topic
    ///   `/skill/set_facial_action_units`) spawns
    ///   `set_facial_action_units(codes: [u8], intensities: [f32])`. It sets
    ///   what `set_expression` sets, the face's expression, and the device
    ///   gives the face to the latest of the two;
    /// - Vizij's `set_viseme` skill (`vizij_face_msgs/SetViseme` on the topic
    ///   `/skill/set_viseme`) spawns `set_viseme(viseme: u8)`: the mouth
    ///   shape at the playhead, for a live producer;
    /// - Vizij's `play_visemes` skill (the **action** `/skill/play_visemes`,
    ///   `vizij_face_msgs/PlayVisemes`) spawns
    ///   `play_visemes(visemes: [u8], starts: [f32], durations: [f32])`: the
    ///   goal's timeline as parallel arrays, times in seconds from its
    ///   origin. A timeline ends, so it is an action, with feedback,
    ///   completion and cancellation;
    /// - `look_at` points (`geometry_msgs/PointStamped`) land as the gaze
    ///   target (a vec3) and frame;
    /// - a streamed viseme lands as its ROS4HRI code on
    ///   `standard/ros4hri/viseme`, with where it falls in the utterance on
    ///   `standard/ros4hri/viseme/time` and how long it is held on
    ///   `standard/ros4hri/viseme/duration` (seconds), all three in one
    ///   change. It comes in either shape a TTS node publishes it in: one
    ///   `hri_msgs/Viseme` on `/tts/viseme`, or an `hri_msgs/Visemes` on
    ///   `/tts/visemes`. Both carry the shape at the audio playhead, so
    ///   the sequence's first viseme is the one that lands; a message holding
    ///   a whole alignment is a timeline, and playing one out over time is a
    ///   viseme player's work rather than a bridge's. This is how a face
    ///   lipsyncs to speech synthesized elsewhere — a `/skill/say` run drives
    ///   the lips from the run itself, never through this topic;
    /// - the utterance being spoken publishes as `std_msgs/String` on
    ///   `/robot_face/speech` from the speech state key
    ///   `standard/ros4hri/speech/text` — what a say run is saying, empty at
    ///   rest — so a subtitle or a transcript follows the voice. Text is not
    ///   commanded through a topic: speaking is the `/skill/say` action;
    /// - the lip shape being spoken publishes as `std_msgs/String` on
    ///   `/robot_face/viseme` from the speech state key
    ///   `standard/ros4hri/speech/viseme` — one of the Vizij standard's 15
    ///   shape names (`aa`, `PP`, …), `sil` at rest — so anything that follows
    ///   the lips (a mirror face, a monitor) reads one topic for the whole
    ///   robot, whichever player is speaking. It is a name, not an
    ///   `hri_msgs/Viseme`: the device reports the shape, and the code, time
    ///   and duration are a stream's, which the face's state does not carry.
    ///   `/robot_face/viseme` is not one of PAL's names;
    /// - the `/skill/look_at` **action** (`interaction_skills/LookAt`) spawns
    ///   the device's `look_at` task run, its goal routed onto the
    ///   `(policy, target, frame)` parameters — the skill plane, for gaze
    ///   work that runs until it converges or is cancelled. The `set_*`
    ///   skills are topics: setting a target state is not a task a client
    ///   follows;
    /// - the rendered face publishes as the `image_transport` pair PAL OS
    ///   documents under `/robot_face/image_raw/*`: the key `display/face`
    ///   as a `sensor_msgs/Image` on `/robot_face/image_raw`, and
    ///   `display/face/compressed` as a `sensor_msgs/CompressedImage` on
    ///   `/robot_face/image_raw/compressed`. One key per transport, because
    ///   a profile is static and a face encodes one way: it writes the key
    ///   of the transport it encodes, and a key never written never
    ///   publishes. [`coverage`](Self::coverage) therefore reports the other
    ///   transport's key as unserved — which is what a consumer of that
    ///   transport finds, so it is the truth of the pair rather than a hole.
    ///
    /// - a closed caption (`hri_actions_msgs/ClosedCaption` on
    ///   `/closed_captions`) lands as its text on
    ///   `standard/ros4hri/caption/text` and its speaker on
    ///   `standard/ros4hri/caption/speaker`, in one change — the speaker as
    ///   the message carries it, `__system__` when the robot itself speaks.
    ///   Showing it is the device's: the topic says what is being said, not
    ///   how long it stays on screen;
    /// - the device's health publishes as a `diagnostic_msgs/DiagnosticArray`
    ///   on `/diagnostics` from the key `diagnostics`, which holds the whole
    ///   message. It is the one outbound endpoint delivered reliably: the
    ///   diagnostics tooling (`diagnostic_aggregator`, `rqt_robot_monitor`)
    ///   subscribes reliably, and a best-effort writer matches no reliable
    ///   reader. A report is a second apart, so a reader that cannot keep up
    ///   costs the writer little.
    ///
    /// REP-155 standardises the perception of humans and says nothing of the
    /// robot's own face; the `/robot_face/*` names are PAL's, and they are
    /// what the ROS4HRI tooling (the interaction simulator, `rqt`) reads.
    /// `/closed_captions` and `/diagnostics` are the names the ROS4HRI nodes
    /// and the ROS diagnostics tooling use.
    ///
    /// The preset declares no bulk includes.
    pub fn ros4hri() -> Self {
        let look_at_routes = vec![
            FieldRoute {
                field: "point".into(),
                key: "standard/ros4hri/gaze/target".into(),
            },
            FieldRoute {
                field: "header.frame_id".into(),
                key: "standard/ros4hri/gaze/frame".into(),
            },
        ];
        let speech_routes = vec![FieldRoute {
            field: "data".into(),
            key: "standard/ros4hri/speech/text".into(),
        }];
        let caption_routes = vec![
            FieldRoute {
                field: "text".into(),
                key: "standard/ros4hri/caption/text".into(),
            },
            FieldRoute {
                field: "speaker_id".into(),
                key: "standard/ros4hri/caption/speaker".into(),
            },
        ];
        let spoken_viseme_routes = vec![FieldRoute {
            field: "data".into(),
            key: "standard/ros4hri/speech/viseme".into(),
        }];
        // The two shapes a viseme stream comes in reach the same keys: the
        // viseme alone, or the first of a sequence.
        let viseme_fields = [
            ("value", "standard/ros4hri/viseme"),
            ("time", "standard/ros4hri/viseme/time"),
            ("duration", "standard/ros4hri/viseme/duration"),
        ];
        let viseme_routes = |prefix: &str| -> Vec<FieldRoute> {
            viseme_fields
                .iter()
                .map(|(field, key)| FieldRoute {
                    field: format!("{prefix}{field}"),
                    key: (*key).into(),
                })
                .collect()
        };
        let viseme_sequence_routes = viseme_routes("visemes.0.");
        let viseme_routes = viseme_routes("");
        // Every endpoint takes its flow's default delivery. A command surface
        // is reliable: an expression that is dropped is an instruction the
        // face never carries out. The image and the speech state are sensor
        // data: a frame, an utterance or a lip shape is state, and a slow
        // reader must not stall the renderer.
        let endpoint = |topic: &str, ros_type: &str, flow: Flow, routes: &[FieldRoute]| Endpoint {
            topic: topic.into(),
            ros_type: ros_type.into(),
            flow,
            routes: routes.to_vec(),
            qos: None,
        };
        // An image or a diagnostics report is not fanned out: the key holds
        // the whole message.
        let whole = |key: &str| {
            vec![FieldRoute {
                field: String::new(),
                key: key.into(),
            }]
        };
        // A skill's message fields, each onto the parameter it feeds.
        let routes = |pairs: &[(&str, &str)]| -> Vec<FieldRoute> {
            pairs
                .iter()
                .map(|(field, parameter)| FieldRoute {
                    field: (*field).into(),
                    key: (*parameter).into(),
                })
                .collect()
        };
        let topic_skill = |topic: &str, ros_type: &str, function: &str, pairs: &[(&str, &str)]| {
            TopicSkillBinding {
                topic: topic.into(),
                ros_type: ros_type.into(),
                function: function.into(),
                routes: routes(pairs),
                qos: None,
            }
        };
        Self {
            name: "ros4hri".into(),
            endpoints: vec![
                endpoint(
                    "/robot_face/look_at",
                    "geometry_msgs/PointStamped",
                    Flow::In,
                    &look_at_routes,
                ),
                endpoint("/tts/viseme", "hri_msgs/Viseme", Flow::In, &viseme_routes),
                endpoint(
                    "/tts/visemes",
                    "hri_msgs/Visemes",
                    Flow::In,
                    &viseme_sequence_routes,
                ),
                endpoint(
                    "/closed_captions",
                    "hri_actions_msgs/ClosedCaption",
                    Flow::In,
                    &caption_routes,
                ),
                endpoint(
                    "/robot_face/speech",
                    "std_msgs/String",
                    Flow::Out,
                    &speech_routes,
                ),
                endpoint(
                    "/robot_face/viseme",
                    "std_msgs/String",
                    Flow::Out,
                    &spoken_viseme_routes,
                ),
                endpoint(
                    "/robot_face/image_raw",
                    "sensor_msgs/Image",
                    Flow::Out,
                    &whole("display/face"),
                ),
                endpoint(
                    "/robot_face/image_raw/compressed",
                    "sensor_msgs/CompressedImage",
                    Flow::Out,
                    &whole("display/face/compressed"),
                ),
                Endpoint {
                    qos: Some(Qos::Reliable),
                    ..endpoint(
                        "/diagnostics",
                        "diagnostic_msgs/DiagnosticArray",
                        Flow::Out,
                        &whole("diagnostics"),
                    )
                },
            ],
            includes: Vec::new(),
            actions: vec![
                ActionBinding {
                    action: "/skill/look_at".into(),
                    ros_type: "interaction_skills/LookAt".into(),
                    function: "look_at".into(),
                    goal_routes: vec![
                        FieldRoute {
                            field: "policy".into(),
                            key: "policy".into(),
                        },
                        FieldRoute {
                            field: "target.point".into(),
                            key: "target".into(),
                        },
                        FieldRoute {
                            field: "target.header.frame_id".into(),
                            key: "frame".into(),
                        },
                    ],
                },
                // A timeline routes as parallel arrays, one entry per
                // viseme, its timing in seconds.
                ActionBinding {
                    action: "/skill/play_visemes".into(),
                    ros_type: "vizij_face_msgs/PlayVisemes".into(),
                    function: "play_visemes".into(),
                    goal_routes: routes(&[
                        ("timeline.visemes.*.viseme.value", "visemes"),
                        ("timeline.visemes.*.timing.start", "starts"),
                        ("timeline.visemes.*.timing.duration", "durations"),
                    ]),
                },
                // The speech skill. `person_id` and `group_id` address an
                // audience a face device has no notion of. The goal carries
                // only the text, so the device's `say` takes its other
                // parameters as absent (an optional voice) or supplies them.
                ActionBinding {
                    action: "/skill/say".into(),
                    ros_type: "communication_skills/Say".into(),
                    function: "say".into(),
                    goal_routes: vec![FieldRoute {
                        field: "input".into(),
                        key: "text".into(),
                    }],
                },
            ],
            topic_skills: vec![
                topic_skill(
                    "/skill/set_expression",
                    "interaction_skills/SetExpression",
                    "set_expression",
                    &[
                        ("expression.expression", "expression"),
                        ("expression.valence", "valence"),
                        ("expression.arousal", "arousal"),
                    ],
                ),
                topic_skill(
                    "/skill/set_facial_action_units",
                    "vizij_face_msgs/SetFacialActionUnits",
                    "set_facial_action_units",
                    &[
                        ("facial_action_units.codes", "codes"),
                        ("facial_action_units.intensities", "intensities"),
                    ],
                ),
                topic_skill(
                    "/skill/set_viseme",
                    "vizij_face_msgs/SetViseme",
                    "set_viseme",
                    &[("viseme.value", "viseme")],
                ),
            ],
        }
    }

    /// This profile with `/skill/set_expression` landing on the
    /// `standard/ros4hri/expression/{name,valence,arousal}` keys — a typed
    /// endpoint — instead of spawning the device's `set_expression` task run:
    /// for a face that reads its expression from those keys and implements
    /// no `set_expression` method.
    pub fn with_set_expression_on_keys(mut self) -> Self {
        self.topic_skills
            .retain(|skill| skill.topic != "/skill/set_expression");
        self.endpoints.insert(
            0,
            Endpoint {
                topic: "/skill/set_expression".into(),
                ros_type: "interaction_skills/SetExpression".into(),
                flow: Flow::In,
                routes: [
                    ("expression.expression", "standard/ros4hri/expression/name"),
                    ("expression.valence", "standard/ros4hri/expression/valence"),
                    ("expression.arousal", "standard/ros4hri/expression/arousal"),
                ]
                .iter()
                .map(|(field, key)| FieldRoute {
                    field: (*field).into(),
                    key: (*key).into(),
                })
                .collect(),
                qos: None,
            },
        );
        self
    }

    /// What the device does not serve of this profile: the endpoint route
    /// keys absent from `keys` (its key set), the include globs matching
    /// nothing, and the action and topic-skill bindings whose function is
    /// absent from `functions` (its described method names). Empty means full
    /// coverage.
    pub fn coverage<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str> + Clone,
        functions: impl IntoIterator<Item = &'a str> + Clone,
    ) -> Vec<String> {
        let mut missing = Vec::new();
        for endpoint in &self.endpoints {
            for route in &endpoint.routes {
                if !keys.clone().into_iter().any(|key| key == route.key) {
                    let mut entry = String::new();
                    let _ = write!(
                        entry,
                        "{} ({}): no device key '{}'",
                        endpoint.topic, endpoint.ros_type, route.key
                    );
                    missing.push(entry);
                }
            }
        }
        for include in &self.includes {
            if !keys
                .clone()
                .into_iter()
                .any(|key| glob_match(&include.glob, key))
            {
                missing.push(format!(
                    "include '{}': no matching device key",
                    include.glob
                ));
            }
        }
        for binding in &self.actions {
            if !functions.clone().into_iter().any(|f| f == binding.function) {
                missing.push(format!(
                    "{} ({}): no device method '{}'",
                    binding.action, binding.ros_type, binding.function
                ));
            }
        }
        for binding in &self.topic_skills {
            if !functions.clone().into_iter().any(|f| f == binding.function) {
                missing.push(format!(
                    "{} ({}): no device method '{}'",
                    binding.topic, binding.ros_type, binding.function
                ));
            }
        }
        missing
    }
}

impl Include {
    /// The absolute topic a matching `key` publishes under, `None` when the
    /// key is outside this include. What remains of the key after `strip` is
    /// escaped into valid ROS 2 name tokens as a key's own topic is (see
    /// [`topic_name`](crate::conversions::topic_name)); `prefix` is used
    /// verbatim.
    pub fn rewrite(&self, key: &str) -> Option<String> {
        if !glob_match(&self.glob, key) {
            return None;
        }
        let rest = key.strip_prefix(&self.strip)?;
        Some(format!("{}{}", self.prefix, escape_path(rest)))
    }
}

/// Glob match over `/`-separated key paths: `*` matches exactly one segment,
/// a trailing `**` matches the rest (including nothing). No other wildcards.
pub(crate) fn glob_match(glob: &str, path: &str) -> bool {
    let mut glob_segments = glob.split('/').peekable();
    let mut path_segments = path.split('/');
    loop {
        match glob_segments.next() {
            None => return path_segments.next().is_none(),
            Some("**") if glob_segments.peek().is_none() => return true,
            Some(glob_segment) => match path_segments.next() {
                Some(path_segment) if glob_segment == "*" || glob_segment == path_segment => {}
                _ => return false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_segments_and_tails() {
        assert!(glob_match("standard/ros4hri/**", "standard/ros4hri/au/12"));
        assert!(glob_match("standard/ros4hri/**", "standard/ros4hri"));
        assert!(glob_match(
            "standard/*/gaze/target",
            "standard/ros4hri/gaze/target"
        ));
        assert!(!glob_match(
            "standard/*/gaze",
            "standard/ros4hri/gaze/target"
        ));
        assert!(!glob_match("standard/ros4hri/**", "standard/vizij/x"));
        assert!(!glob_match("standard/ros4hri", "standard/ros4hri/x"));
    }

    #[test]
    fn includes_rewrite_prefixes() {
        let include = Include {
            qos: None,
            glob: "standard/ros4hri/viseme/**".into(),
            strip: "standard/ros4hri/".into(),
            prefix: "/robot_face/state/".into(),
            flow: Flow::Out,
        };
        assert_eq!(
            include.rewrite("standard/ros4hri/viseme/aa").as_deref(),
            Some("/robot_face/state/viseme/aa"),
        );
        assert_eq!(include.rewrite("standard/vizij/viseme/aa"), None);
        assert_eq!(
            include
                .rewrite("standard/ros4hri/viseme/pose_b.weight")
                .as_deref(),
            Some("/robot_face/state/viseme/pose_b_weight"),
        );
    }

    #[test]
    fn ros4hri_preset_serves_the_face_topics() {
        let profile = ExposureProfile::ros4hri();
        let topics: Vec<&str> = profile.endpoints.iter().map(|e| e.topic.as_str()).collect();
        for expected in [
            "/robot_face/look_at",
            "/tts/viseme",
            "/tts/visemes",
            "/robot_face/speech",
            "/robot_face/viseme",
            "/robot_face/image_raw",
            "/robot_face/image_raw/compressed",
            "/closed_captions",
            "/diagnostics",
        ] {
            assert!(topics.contains(&expected), "missing {expected}");
        }
        // The commands and the captions flow in; the image, the speech state
        // and the diagnostics flow out.
        for endpoint in &profile.endpoints {
            let expected = if endpoint.topic.starts_with("/robot_face/image_raw")
                || endpoint.topic == "/robot_face/speech"
                || endpoint.topic == "/robot_face/viseme"
                || endpoint.topic == "/diagnostics"
            {
                Flow::Out
            } else {
                Flow::In
            };
            assert_eq!(endpoint.flow, expected, "{}", endpoint.topic);
        }
    }

    /// Both viseme shapes a TTS node publishes reach the same keys — the
    /// code, its time and its duration — so a face lipsyncs to either
    /// without knowing which it is fed.
    #[test]
    fn ros4hri_preset_takes_a_viseme_in_either_shape() {
        let profile = ExposureProfile::ros4hri();
        for (topic, ros_type, prefix) in [
            ("/tts/viseme", "hri_msgs/Viseme", ""),
            ("/tts/visemes", "hri_msgs/Visemes", "visemes.0."),
        ] {
            let endpoint = profile
                .endpoints
                .iter()
                .find(|e| e.topic == topic)
                .unwrap_or_else(|| panic!("{topic} is in the preset"));
            assert_eq!(endpoint.ros_type, ros_type, "{topic}");
            assert_eq!(endpoint.flow, Flow::In, "{topic}");
            let routes: Vec<(&str, &str)> = endpoint
                .routes
                .iter()
                .map(|route| (route.field.as_str(), route.key.as_str()))
                .collect();
            assert_eq!(
                routes,
                [
                    (format!("{prefix}value"), "standard/ros4hri/viseme"),
                    (format!("{prefix}time"), "standard/ros4hri/viseme/time"),
                    (
                        format!("{prefix}duration"),
                        "standard/ros4hri/viseme/duration"
                    ),
                ]
                .iter()
                .map(|(field, key)| (field.as_str(), *key))
                .collect::<Vec<_>>(),
                "{topic}"
            );
        }
    }

    /// A caption lands as its text and its speaker, each on its own key,
    /// delivered reliably like every inbound surface: a dropped caption is
    /// text the face never shows.
    #[test]
    fn ros4hri_preset_takes_a_closed_caption_as_text_and_speaker() {
        let profile = ExposureProfile::ros4hri();
        let endpoint = profile
            .endpoints
            .iter()
            .find(|e| e.topic == "/closed_captions")
            .expect("/closed_captions is in the preset");
        assert_eq!(endpoint.ros_type, "hri_actions_msgs/ClosedCaption");
        assert_eq!(endpoint.flow, Flow::In);
        assert_eq!(
            endpoint.qos, None,
            "an inbound surface takes the reliable default"
        );
        let routes: Vec<(&str, &str)> = endpoint
            .routes
            .iter()
            .map(|r| (r.field.as_str(), r.key.as_str()))
            .collect();
        assert_eq!(
            routes,
            [
                ("text", "standard/ros4hri/caption/text"),
                ("speaker_id", "standard/ros4hri/caption/speaker"),
            ]
        );
    }

    #[test]
    fn ros4hri_preset_binds_the_standard_skills() {
        let profile = ExposureProfile::ros4hri();
        let [look_at, play_visemes, say] = profile.actions.as_slice() else {
            panic!(
                "the gaze, viseme and speech skills, got {:?}",
                profile.actions
            );
        };
        assert_eq!(look_at.action, "/skill/look_at");
        assert_eq!(look_at.ros_type, "interaction_skills/LookAt");
        assert_eq!(look_at.function, "look_at");
        let params: Vec<&str> = look_at.goal_routes.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(params, ["policy", "target", "frame"]);

        assert_eq!(play_visemes.action, "/skill/play_visemes");
        assert_eq!(play_visemes.ros_type, "vizij_face_msgs/PlayVisemes");
        assert_eq!(play_visemes.function, "play_visemes");
        let routes: Vec<(&str, &str)> = play_visemes
            .goal_routes
            .iter()
            .map(|r| (r.field.as_str(), r.key.as_str()))
            .collect();
        assert_eq!(
            routes,
            [
                ("timeline.visemes.*.viseme.value", "visemes"),
                ("timeline.visemes.*.timing.start", "starts"),
                ("timeline.visemes.*.timing.duration", "durations"),
            ]
        );

        assert_eq!(say.action, "/skill/say");
        assert_eq!(say.ros_type, "communication_skills/Say");
        assert_eq!(say.function, "say");
        // The utterance is the one field a face device reads from the goal.
        let params: Vec<&str> = say.goal_routes.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(params, ["text"]);
    }

    /// The `set_*` skills ride topics, each spawning the face method of its
    /// name with the message's content routed onto its parameters.
    #[test]
    fn ros4hri_preset_serves_the_set_skills_on_topics() {
        let profile = ExposureProfile::ros4hri();
        // (topic, type, function, routes)
        type Skill<'a> = (&'a str, &'a str, &'a str, Vec<(&'a str, &'a str)>);
        let skills: Vec<Skill> = profile
            .topic_skills
            .iter()
            .map(|skill| {
                (
                    skill.topic.as_str(),
                    skill.ros_type.as_str(),
                    skill.function.as_str(),
                    skill
                        .routes
                        .iter()
                        .map(|r| (r.field.as_str(), r.key.as_str()))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            skills,
            [
                (
                    "/skill/set_expression",
                    "interaction_skills/SetExpression",
                    "set_expression",
                    vec![
                        ("expression.expression", "expression"),
                        ("expression.valence", "valence"),
                        ("expression.arousal", "arousal"),
                    ],
                ),
                (
                    "/skill/set_facial_action_units",
                    "vizij_face_msgs/SetFacialActionUnits",
                    "set_facial_action_units",
                    vec![
                        ("facial_action_units.codes", "codes"),
                        ("facial_action_units.intensities", "intensities"),
                    ],
                ),
                (
                    "/skill/set_viseme",
                    "vizij_face_msgs/SetViseme",
                    "set_viseme",
                    vec![("viseme.value", "viseme")],
                ),
            ]
        );
        // A skill topic is not also a typed endpoint: one message, one effect.
        for skill in &profile.topic_skills {
            assert!(
                !profile.endpoints.iter().any(|e| e.topic == skill.topic),
                "{} is both",
                skill.topic
            );
        }
    }

    /// The switch back to the expression keys moves `/skill/set_expression`
    /// from the skill plane to a typed endpoint, and touches nothing else.
    #[test]
    fn set_expression_can_land_on_the_expression_keys_instead() {
        let profile = ExposureProfile::ros4hri().with_set_expression_on_keys();
        assert!(!profile
            .topic_skills
            .iter()
            .any(|s| s.topic == "/skill/set_expression"));
        assert_eq!(profile.topic_skills.len(), 2);
        let endpoint = profile
            .endpoints
            .iter()
            .find(|e| e.topic == "/skill/set_expression")
            .expect("the expression endpoint");
        assert_eq!(endpoint.ros_type, "interaction_skills/SetExpression");
        assert_eq!(endpoint.flow, Flow::In);
        let keys: Vec<&str> = endpoint.routes.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "standard/ros4hri/expression/name",
                "standard/ros4hri/expression/valence",
                "standard/ros4hri/expression/arousal",
            ]
        );
    }

    #[test]
    fn coverage_reports_unserved_surfaces() {
        let profile = ExposureProfile::ros4hri();
        // A face serving every route key and every skill method covers the
        // preset fully.
        let functions = [
            "look_at",
            "play_visemes",
            "say",
            "set_expression",
            "set_facial_action_units",
            "set_viseme",
        ];
        let keys: Vec<String> = profile
            .endpoints
            .iter()
            .flat_map(|e| e.routes.iter().map(|r| r.key.clone()))
            .collect();
        assert!(profile
            .coverage(keys.iter().map(String::as_str), functions)
            .is_empty());
        // Dropping the gaze target surfaces exactly the look_at hole.
        let partial: Vec<&str> = keys
            .iter()
            .map(String::as_str)
            .filter(|k| !k.ends_with("gaze/target"))
            .collect();
        let missing = profile.coverage(partial, functions);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("/robot_face/look_at"));
        assert!(missing[0].contains("gaze/target"));
        // A device serving no skill method misses the whole skill plane, on
        // actions and on topics.
        let missing = profile.coverage(keys.iter().map(String::as_str), []);
        assert_eq!(missing.len(), functions.len(), "{missing:?}");
        for skill in [
            "/skill/look_at",
            "/skill/play_visemes",
            "/skill/say",
            "/skill/set_expression",
            "/skill/set_facial_action_units",
            "/skill/set_viseme",
        ] {
            assert!(missing.iter().any(|m| m.contains(skill)), "{skill}");
        }
        // A face encodes one transport, so the other's key is reported as
        // unserved — the truth of the pair, named by topic and type.
        let png_only: Vec<&str> = keys
            .iter()
            .map(String::as_str)
            .filter(|k| *k != "display/face")
            .collect();
        let missing = profile.coverage(png_only, functions);
        assert_eq!(
            missing,
            ["/robot_face/image_raw (sensor_msgs/Image): no device key 'display/face'"]
        );
    }
}
