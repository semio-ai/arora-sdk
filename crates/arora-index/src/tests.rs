use super::*;

/// Pins the SHA-256 algorithm and lowercase wire format against known answers.
#[test]
fn hashes_match_known_answers() {
    assert_eq!(
        content_hash(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        content_hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// Kinds are strings, including future kinds; constants pin the shared vocabulary.
#[test]
fn kinds_round_trip_without_a_closed_vocabulary() {
    for (kind, text) in [
        (Kind::MODEL, "model"),
        (Kind::FACE, "face"),
        (Kind::ANIMATION, "animation"),
        (Kind::MODULE, "module"),
        (Kind::TYPE_RECORD, "type-record"),
        (Kind::BEHAVIOR, "behavior"),
        (Kind::from("future-kind"), "future-kind"),
    ] {
        assert_eq!(kind.as_str(), text);
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<Kind>(&json).unwrap(), kind);
    }
}
