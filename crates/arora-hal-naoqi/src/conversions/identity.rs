//! What a NAOqi robot says about itself, as an Arora HAL description.
//!
//! The model family comes from `ALRobotModel.getRobotType` (NAOqi 2.5 and later) or
//! `ALMemory`'s `RobotConfig/Body/Type` (every version); the hardware version from
//! `ALMemory`'s `RobotConfig/Body/BaseVersion` and `RobotConfig/Head/BaseVersion` (a NAO's
//! head and body are versioned apart: a V5 "Evolution" head may sit on a V4 body) and the body
//! type of `ALMotion.getRobotConfig` (`naoH25`, `naoT14`…); the software version from
//! `ALSystem.systemVersion`.

use qi::value::Value as QiValue;

use super::values::qi_list;

/// The Arora model family of a NAOqi robot type: `nao`, `pepper` (which NAOqi names
/// `Juliette` in its configuration), `romeo`, or the type itself in lower case.
pub(crate) fn model_family(robot_type: &str) -> String {
    match robot_type.trim().to_lowercase().as_str() {
        "juliette" | "pepper" => "pepper".to_string(),
        other => other.to_string(),
    }
}

/// The hardware version: the body's base version, then the head's when it differs and the
/// body type, as `V4.0 (head VEvolution, naoH25)`.
pub(crate) fn hardware_version(
    body: Option<&str>,
    head: Option<&str>,
    body_type: Option<&str>,
) -> Option<String> {
    let body = body.filter(|version| !version.is_empty());
    let head = head
        .filter(|version| !version.is_empty())
        .filter(|head| Some(*head) != body)
        .map(|head| format!("head {head}"));
    let details: Vec<String> = head
        .into_iter()
        .chain(
            body_type
                .filter(|body_type| !body_type.is_empty())
                .map(str::to_string),
        )
        .collect();
    match (body, details.is_empty()) {
        (Some(body), true) => Some(body.to_string()),
        (Some(body), false) => Some(format!("{body} ({})", details.join(", "))),
        (None, false) => Some(details.join(", ")),
        (None, true) => None,
    }
}

/// The text of an entry of `ALMotion.getRobotConfig`'s answer: a pair of lists, the names
/// of the entries and their values.
pub(crate) fn robot_config_entry(robot_config: QiValue<'static>, name: &str) -> Option<String> {
    let mut lists = qi_list(robot_config)?.into_iter();
    let names = qi_list(lists.next()?)?;
    let values = qi_list(lists.next()?)?;
    let index = names.iter().position(|entry| qi_str(entry) == Some(name))?;
    values.get(index).and_then(qi_str).map(str::to_string)
}

fn qi_str<'v>(value: &'v QiValue<'_>) -> Option<&'v str> {
    match value {
        QiValue::String(text) => text.as_str(),
        QiValue::Dynamic(inner) => qi_str(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_families() {
        assert_eq!(model_family("Nao"), "nao");
        assert_eq!(model_family("Juliette"), "pepper");
        assert_eq!(model_family("Pepper"), "pepper");
        assert_eq!(model_family("Romeo"), "romeo");
    }

    #[test]
    fn hardware_versions() {
        // A NAO V4 body with a V5 head, under NAOqi 2.1.
        assert_eq!(
            hardware_version(Some("V4.0"), Some("VEvolution"), Some("naoH25")).as_deref(),
            Some("V4.0 (head VEvolution, naoH25)")
        );
        assert_eq!(
            hardware_version(Some("V6"), Some("V6"), None).as_deref(),
            Some("V6")
        );
        assert_eq!(
            hardware_version(None, Some("V5"), None).as_deref(),
            Some("head V5")
        );
        assert_eq!(hardware_version(None, None, None), None);
    }

    #[test]
    fn robot_config_entries() {
        let text = |s: &str| QiValue::String(s.to_string().into());
        let config = QiValue::Dynamic(Box::new(QiValue::List(vec![
            QiValue::List(vec![text("Model Type"), text("Head Version")]),
            QiValue::List(vec![
                QiValue::Dynamic(Box::new(text("naoH25"))),
                text("VERSION_40"),
            ]),
        ])));
        assert_eq!(
            robot_config_entry(config.clone(), "Model Type").as_deref(),
            Some("naoH25")
        );
        assert_eq!(robot_config_entry(config, "Laser"), None);
    }
}
