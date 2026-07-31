//! Exposure profiles: which device keys a bridge exposes to ROS 2, under
//! which absolute names, and with which message types (ARORA-86, Track A5 of
//! the ROS4HRI face plan).
//!
//! A profile has two planes, matching how ROS4HRI consumes a face:
//!
//! - [`Endpoint`]s bind an absolute topic to a registered message type and
//!   fan its fields out over device keys ([`FieldRoute`]) — the skill
//!   surfaces, where a structured message maps to several keys and no string
//!   rewrite could express the relation.
//! - [`Include`]s select bulk data keys by **glob** (`*` one segment, `**`
//!   the rest — regex was rejected) and rewrite their prefix into an absolute
//!   topic name — the state plane, where key names and topic names correspond
//!   one-to-one.
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

/// A named set of [`Endpoint`]s and [`Include`]s — everything a deployment
/// says with "expose this face as `<profile>`".
#[derive(Debug, Clone)]
pub struct ExposureProfile {
    pub name: String,
    pub endpoints: Vec<Endpoint>,
    pub includes: Vec<Include>,
}

impl ExposureProfile {
    /// The ROS4HRI face surface, serving both incumbent name sets — PAL
    /// (`/robot_face/*`) and IIIA (`/expressive_face/*`) — out of the box:
    ///
    /// - expression commands (`hri_msgs/Expression`) fan out to the
    ///   `standard/ros4hri/expression/*` keys the face standard reads;
    /// - `look_at` points (`geometry_msgs/PointStamped`) land as the gaze
    ///   target (a vec3) and frame;
    /// - speech text (`std_msgs/String`) lands on the lipsync feed key.
    ///
    /// The IIIA `/skill/set_expression` endpoint
    /// (`interaction_skills/SetExpression`) joins once that package is
    /// vendored in `arora-msgs-ros2`; the outbound plane (image,
    /// diagnostics) is typed and tracked separately, so the preset declares
    /// no bulk includes.
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
        }
    }

    /// What `keys` (a device's key set) does not serve of this profile: the
    /// endpoint route keys absent from the set, and the include globs
    /// matching nothing. Empty means full coverage.
    pub fn coverage<'a>(&self, keys: impl IntoIterator<Item = &'a str> + Clone) -> Vec<String> {
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
    fn coverage_reports_unserved_surfaces() {
        let profile = ExposureProfile::ros4hri();
        // A face serving every route key covers the preset fully.
        let keys: Vec<String> = profile
            .endpoints
            .iter()
            .flat_map(|e| e.routes.iter().map(|r| r.key.clone()))
            .collect();
        assert!(profile.coverage(keys.iter().map(String::as_str)).is_empty());
        // Dropping the gaze target surfaces exactly the look_at holes.
        let partial: Vec<&str> = keys
            .iter()
            .map(String::as_str)
            .filter(|k| !k.ends_with("gaze/target"))
            .collect();
        let missing = profile.coverage(partial);
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing.iter().all(|m| m.contains("gaze/target")));
    }
}
