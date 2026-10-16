use crate::version::{Range, Version};
use crate::{valid_hash, Error, Release, SourceError};

/// Select the newest non-deprecated release with the requested artifact.
/// Invalid versions and absent or empty artifact hashes are skipped. Equal
/// versions keep listing order. A malformed selected hash fails without
/// selecting an older release. Module keys conventionally include `interface`,
/// `wasm` and `native/<target-triple>`; the index does not restrict keys.
pub fn select_release<'a>(
    releases: &'a [Release],
    scoped_name: &str,
    range: &str,
    artifact_key: &str,
) -> Result<&'a Release, Error> {
    validate_name(scoped_name)?;
    let parsed = Range::parse(scoped_name, range)?;
    let mut selected: Option<(Version, &Release)> = None;
    for release in releases {
        let Some(version) = Version::parse(&release.version) else {
            continue;
        };
        if release.deprecated
            || !parsed.contains(version)
            || release
                .artifacts
                .get(artifact_key)
                .is_none_or(|a| a.content_hash.is_empty())
        {
            continue;
        }
        if selected.is_none_or(|(current, _)| version > current) {
            selected = Some((version, release));
        }
    }
    let (_, release) = selected.ok_or_else(|| Error::NoRelease {
        scoped_name: scoped_name.into(),
        range: range.into(),
    })?;
    if !valid_hash(&release.artifacts[artifact_key].content_hash) {
        return Err(SourceError::Malformed(format!(
            "{}: release {}: invalid hash for {artifact_key}",
            scoped_name, release.release_id
        ))
        .into());
    }
    Ok(release)
}

/// Names are scoped addresses. Reject path components as well as protocol separators.
pub(crate) fn validate_name(name: &str) -> Result<(), Error> {
    let parts = name.strip_prefix('@').and_then(|s| s.split_once('/'));
    if parts.is_none_or(|(scope, name)| {
        [scope, name].iter().any(|s| {
            s.is_empty()
                || matches!(*s, "." | "..")
                || s.contains(['/', ':', '\\'])
                || s.chars().any(char::is_control)
        })
    }) {
        return Err(Error::InvalidName {
            scoped_name: name.into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArtifactRef, Kind};
    use std::collections::BTreeMap;

    fn release(id: &str, version: &str) -> Release {
        Release {
            scoped_name: "@test/model".into(),
            version: version.into(),
            release_id: id.into(),
            kind: Kind::MODEL,
            deprecated: false,
            private: false,
            family: None,
            artifacts: BTreeMap::from([(
                "primary".into(),
                ArtifactRef {
                    content_hash: "a".repeat(64),
                    size: None,
                },
            )]),
        }
    }

    /// Selection skips invalid, deprecated and missing candidates and keeps the first numeric tie.
    #[test]
    fn selection_is_stable_and_filters_candidates() {
        let mut releases = vec![
            release("first", "v2.10.0"),
            release("tie", "2.10.0"),
            release("older", "2.9.0"),
        ];
        let mut deprecated = release("deprecated", "3.0.0");
        deprecated.deprecated = true;
        let mut absent = release("absent", "4.0.0");
        absent.artifacts.clear();
        let mut empty = release("empty", "5.0.0");
        empty
            .artifacts
            .get_mut("primary")
            .unwrap()
            .content_hash
            .clear();
        releases.extend([
            deprecated,
            absent,
            empty,
            release("invalid", "9.0"),
            release("below-floor", "0.9.9"),
        ]);
        assert_eq!(
            select_release(&releases, "@test/model", "*", "primary")
                .unwrap()
                .release_id,
            "first"
        );
        assert_eq!(
            select_release(&releases, "@test/model", "~2.9.0", "primary")
                .unwrap()
                .release_id,
            "older"
        );
        assert!(matches!(
            select_release(&releases, "@test/model", "^6.0.0", "primary"),
            Err(Error::NoRelease { .. })
        ));
        releases[0]
            .artifacts
            .get_mut("primary")
            .unwrap()
            .content_hash = "BAD".into();
        assert!(matches!(
            select_release(&releases, "@test/model", "*", "primary"),
            Err(Error::Source(SourceError::Malformed(_)))
        ));
    }

    /// Artifact keys are independent; a newer executable cannot hide an older interface.
    #[test]
    fn artifact_keys_select_independently() {
        let mut older = release("interface", "1.0.0");
        older
            .artifacts
            .insert("interface".into(), older.artifacts["primary"].clone());
        let mut newer = release("executable", "2.0.0");
        newer.artifacts.insert(
            "native/aarch64-apple-darwin".into(),
            newer.artifacts["primary"].clone(),
        );
        let releases = [older, newer];
        assert_eq!(
            select_release(&releases, "@test/model", "*", "interface")
                .unwrap()
                .release_id,
            "interface"
        );
        assert_eq!(
            select_release(&releases, "@test/model", "*", "native/aarch64-apple-darwin")
                .unwrap()
                .release_id,
            "executable"
        );
        let restored: Release =
            serde_json::from_value(serde_json::to_value(&releases[1]).unwrap()).unwrap();
        assert_eq!(restored.artifacts, releases[1].artifacts);
    }

    /// Names cannot escape the source root or enter another protocol namespace.
    #[test]
    fn scoped_names() {
        for name in [
            "plain",
            "@/name",
            "@scope/",
            "@a/b/c",
            "@a:b/c",
            "@../secret",
            "@a/..",
            "@a/\\b",
            "@a/b\n",
        ] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        validate_name("@scope/my-model").unwrap();
    }
}
