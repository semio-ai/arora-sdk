//! Exposure profiles: which device keys a bridge exposes to ROS 2, under
//! which absolute names, and with which message types (ARORA-86, Track A5 of
//! the ROS4HRI face plan).
//!
//! A profile has three planes, matching how ROS4HRI consumes a face:
//!
//! - [`Endpoint`]s bind an absolute topic to a registered message type and
//!   fan its fields out over device keys ([`FieldRoute`]) — the command
//!   surfaces, where a structured message maps to several keys and no string
//!   rewrite could express the relation.
//! - [`Include`]s select bulk data keys by **glob** (`*` one segment, `**`
//!   the rest — regex was rejected) and rewrite their prefix into an absolute
//!   topic name — the state plane, where key names and topic names correspond
//!   one-to-one.
//! - [`ActionBinding`]s bind an absolute ROS 2 **action** to a device task-run
//!   method — the skill plane, where long-running, cancellable work rides a
//!   standard `.action` contract (`interaction_skills/LookAt`) instead of a
//!   topic.
//!
//! [`ExposureProfile::coverage`] reports what a device does not serve, so a
//! deployment can check a face against the profile it enables instead of
//! discovering holes topic by topic.

use std::fmt::Write as _;

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
    /// Absolute ROS topic name, e.g. `/robot_face/expression`.
    pub topic: String,
    /// Registered ROS message name, e.g. `hri_msgs/Expression`.
    pub ros_type: String,
    pub flow: Flow,
    /// Field fan-out. To route the whole message onto one key, declare one
    /// route with an empty `field`.
    pub routes: Vec<FieldRoute>,
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
}

/// A profile-declared ROS 2 action bound to a device task-run method: the
/// exterior contract of a **skill** — a standard `.action` type served on a
/// well-known name — systematically associated with the behavior that
/// implements it, the association checked at startup against the device's
/// described methods.
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
    /// parameter it becomes. Every parameter must be routed.
    pub goal_routes: Vec<FieldRoute>,
}

/// A named set of [`Endpoint`]s, [`Include`]s and [`ActionBinding`]s —
/// everything a deployment says with "expose this face as `<profile>`".
#[derive(Debug, Clone)]
pub struct ExposureProfile {
    pub name: String,
    pub endpoints: Vec<Endpoint>,
    pub includes: Vec<Include>,
    pub actions: Vec<ActionBinding>,
}

impl ExposureProfile {
    /// The ROS4HRI face surface, serving both incumbent name sets — PAL
    /// (`/robot_face/*`) and IIIA (`/expressive_face/*`) — out of the box:
    ///
    /// - expression commands (`hri_msgs/Expression`) fan out to the
    ///   `standard/ros4hri/expression/*` keys the face standard reads;
    /// - `look_at` points (`geometry_msgs/PointStamped`) land as the gaze
    ///   target (a vec3) and frame;
    /// - speech text (`std_msgs/String`) lands on the lipsync feed key;
    /// - the `/skill/look_at` **action** (`interaction_skills/LookAt`) spawns
    ///   the device's `look_at` task run, its goal routed onto the
    ///   `(policy, target, frame)` parameters — the skill plane, for gaze
    ///   work that runs until it converges or is cancelled. `set_expression`
    ///   stays a topic: setting a target state is not a task.
    ///
    /// The outbound plane (image, diagnostics) is typed and tracked
    /// separately, so the preset declares no bulk includes.
    pub fn ros4hri() -> Self {
        let expression_routes = vec![
            FieldRoute {
                field: "expression".into(),
                key: "standard/ros4hri/expression/name".into(),
            },
            FieldRoute {
                field: "valence".into(),
                key: "standard/ros4hri/expression/valence".into(),
            },
            FieldRoute {
                field: "arousal".into(),
                key: "standard/ros4hri/expression/arousal".into(),
            },
        ];
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
        let endpoint = |topic: &str, ros_type: &str, routes: &Vec<FieldRoute>| Endpoint {
            topic: topic.into(),
            ros_type: ros_type.into(),
            flow: Flow::In,
            routes: routes.clone(),
        };
        Self {
            name: "ros4hri".into(),
            endpoints: vec![
                endpoint(
                    "/robot_face/expression",
                    "hri_msgs/Expression",
                    &expression_routes,
                ),
                endpoint(
                    "/robot_face/look_at",
                    "geometry_msgs/PointStamped",
                    &look_at_routes,
                ),
                endpoint(
                    "/expressive_face/look_at",
                    "geometry_msgs/PointStamped",
                    &look_at_routes,
                ),
                endpoint("/robot_face/tts", "std_msgs/String", &speech_routes),
                endpoint("/expressive_face/speech", "std_msgs/String", &speech_routes),
            ],
            includes: Vec::new(),
            actions: vec![ActionBinding {
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
            }],
        }
    }

    /// What the device does not serve of this profile: the endpoint route
    /// keys absent from `keys` (its key set), the include globs matching
    /// nothing, and the action bindings whose function is absent from
    /// `functions` (its described method names). Empty means full coverage.
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
        missing
    }
}

impl Include {
    /// The absolute topic a matching `key` publishes under, `None` when the
    /// key is outside this include.
    pub fn rewrite(&self, key: &str) -> Option<String> {
        if !glob_match(&self.glob, key) {
            return None;
        }
        let rest = key.strip_prefix(&self.strip)?;
        Some(format!("{}{}", self.prefix, rest))
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
    }

    #[test]
    fn ros4hri_preset_serves_both_name_sets() {
        let profile = ExposureProfile::ros4hri();
        let topics: Vec<&str> = profile.endpoints.iter().map(|e| e.topic.as_str()).collect();
        for expected in [
            "/robot_face/expression",
            "/robot_face/look_at",
            "/expressive_face/look_at",
            "/robot_face/tts",
            "/expressive_face/speech",
        ] {
            assert!(topics.contains(&expected), "missing {expected}");
        }
        assert!(profile.endpoints.iter().all(|e| e.flow == Flow::In));
    }

    #[test]
    fn ros4hri_preset_binds_the_look_at_skill() {
        let profile = ExposureProfile::ros4hri();
        let [binding] = profile.actions.as_slice() else {
            panic!("one skill binding, got {:?}", profile.actions);
        };
        assert_eq!(binding.action, "/skill/look_at");
        assert_eq!(binding.ros_type, "interaction_skills/LookAt");
        assert_eq!(binding.function, "look_at");
        let params: Vec<&str> = binding.goal_routes.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(params, ["policy", "target", "frame"]);
    }

    #[test]
    fn coverage_reports_unserved_surfaces() {
        let profile = ExposureProfile::ros4hri();
        // A face serving every route key and the look_at method covers the
        // preset fully.
        let keys: Vec<String> = profile
            .endpoints
            .iter()
            .flat_map(|e| e.routes.iter().map(|r| r.key.clone()))
            .collect();
        assert!(profile
            .coverage(keys.iter().map(String::as_str), ["look_at"])
            .is_empty());
        // Dropping the gaze target surfaces exactly the look_at holes.
        let partial: Vec<&str> = keys
            .iter()
            .map(String::as_str)
            .filter(|k| !k.ends_with("gaze/target"))
            .collect();
        let missing = profile.coverage(partial, ["look_at"]);
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing.iter().all(|m| m.contains("gaze/target")));
        // A device without the look_at method misses the skill plane.
        let missing = profile.coverage(keys.iter().map(String::as_str), []);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("/skill/look_at"));
    }
}
