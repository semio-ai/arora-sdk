//! How Arora spells identifiers.
//!
//! An Arora identifier is a UUID. Its canonical spelling — in `module.yaml`,
//! records, the wire, and every other language — is the hyphenated hex form.
//! In Rust source, where an identifier is pinned by hand in an attribute, the
//! same 128 bits may be spelled as **13 emoji**: the UUID as a big-endian
//! integer, written in base 1024 over a fixed alphabet, most significant digit
//! first. Thirteen digits carry 130 bits, so the first digit uses only 8 of
//! them (its index is below 256).
//!
//! [`parse`] accepts either spelling; [`encode`] produces the emoji one;
//! [`decode`] reads it back. The emoji form exists to be recognizable at a
//! glance in a declaration or a diff, where two hex UUIDs are noise — it is a
//! spelling, never a second identity.
//!
//! # The alphabet
//!
//! [`ALPHABET`] is part of the encoding: once published it does not change.
//! Its 1024 entries are the single code points that Unicode 16's
//! `emoji-data.txt` lists with `Emoji_Presentation` (so they render as emoji
//! with no `U+FE0F` selector that an editor could drop), that are not emoji
//! modifiers, components or regional indicators, and that Emoji 12.0 or
//! earlier introduced (fonts from 2019 on show them) — 1058 code points, in
//! code-point order, minus the 24 clock faces (`U+1F550..=U+1F567`) and the 10
//! moon phases (`U+1F311..=U+1F31A`), whose near-identical glyphs defeat the
//! purpose.

#![forbid(unsafe_code)]

use std::fmt;

pub use uuid::Uuid;

/// The number of emoji in an encoded identifier.
pub const LEN: usize = 13;

/// The base-1024 digit alphabet, in code-point order. See the crate docs for
/// what is in it and why it never changes.
pub const ALPHABET: [char; 1024] = [
    '⌚', '⌛', '⏩', '⏪', '⏫', '⏬', '⏰', '⏳', '◽', '◾', '☔', '☕', '♈', '♉', '♊', '♋',
    '♌', '♍', '♎', '♏', '♐', '♑', '♒', '♓', '♿', '⚓', '⚡', '⚪', '⚫', '⚽', '⚾', '⛄',
    '⛅', '⛎', '⛔', '⛪', '⛲', '⛳', '⛵', '⛺', '⛽', '✅', '✊', '✋', '✨', '❌', '❎', '❓',
    '❔', '❕', '❗', '➕', '➖', '➗', '➰', '➿', '⬛', '⬜', '⭐', '⭕', '🀄', '🃏', '🆎', '🆑',
    '🆒', '🆓', '🆔', '🆕', '🆖', '🆗', '🆘', '🆙', '🆚', '🈁', '🈚', '🈯', '🈲', '🈳', '🈴', '🈵',
    '🈶', '🈸', '🈹', '🈺', '🉐', '🉑', '🌀', '🌁', '🌂', '🌃', '🌄', '🌅', '🌆', '🌇', '🌈', '🌉',
    '🌊', '🌋', '🌌', '🌍', '🌎', '🌏', '🌐', '🌛', '🌜', '🌝', '🌞', '🌟', '🌠', '🌭', '🌮', '🌯',
    '🌰', '🌱', '🌲', '🌳', '🌴', '🌵', '🌷', '🌸', '🌹', '🌺', '🌻', '🌼', '🌽', '🌾', '🌿', '🍀',
    '🍁', '🍂', '🍃', '🍄', '🍅', '🍆', '🍇', '🍈', '🍉', '🍊', '🍋', '🍌', '🍍', '🍎', '🍏', '🍐',
    '🍑', '🍒', '🍓', '🍔', '🍕', '🍖', '🍗', '🍘', '🍙', '🍚', '🍛', '🍜', '🍝', '🍞', '🍟', '🍠',
    '🍡', '🍢', '🍣', '🍤', '🍥', '🍦', '🍧', '🍨', '🍩', '🍪', '🍫', '🍬', '🍭', '🍮', '🍯', '🍰',
    '🍱', '🍲', '🍳', '🍴', '🍵', '🍶', '🍷', '🍸', '🍹', '🍺', '🍻', '🍼', '🍾', '🍿', '🎀', '🎁',
    '🎂', '🎃', '🎄', '🎅', '🎆', '🎇', '🎈', '🎉', '🎊', '🎋', '🎌', '🎍', '🎎', '🎏', '🎐', '🎑',
    '🎒', '🎓', '🎠', '🎡', '🎢', '🎣', '🎤', '🎥', '🎦', '🎧', '🎨', '🎩', '🎪', '🎫', '🎬', '🎭',
    '🎮', '🎯', '🎰', '🎱', '🎲', '🎳', '🎴', '🎵', '🎶', '🎷', '🎸', '🎹', '🎺', '🎻', '🎼', '🎽',
    '🎾', '🎿', '🏀', '🏁', '🏂', '🏃', '🏄', '🏅', '🏆', '🏇', '🏈', '🏉', '🏊', '🏏', '🏐', '🏑',
    '🏒', '🏓', '🏠', '🏡', '🏢', '🏣', '🏤', '🏥', '🏦', '🏧', '🏨', '🏩', '🏪', '🏫', '🏬', '🏭',
    '🏮', '🏯', '🏰', '🏴', '🏸', '🏹', '🏺', '🐀', '🐁', '🐂', '🐃', '🐄', '🐅', '🐆', '🐇', '🐈',
    '🐉', '🐊', '🐋', '🐌', '🐍', '🐎', '🐏', '🐐', '🐑', '🐒', '🐓', '🐔', '🐕', '🐖', '🐗', '🐘',
    '🐙', '🐚', '🐛', '🐜', '🐝', '🐞', '🐟', '🐠', '🐡', '🐢', '🐣', '🐤', '🐥', '🐦', '🐧', '🐨',
    '🐩', '🐪', '🐫', '🐬', '🐭', '🐮', '🐯', '🐰', '🐱', '🐲', '🐳', '🐴', '🐵', '🐶', '🐷', '🐸',
    '🐹', '🐺', '🐻', '🐼', '🐽', '🐾', '👀', '👂', '👃', '👄', '👅', '👆', '👇', '👈', '👉', '👊',
    '👋', '👌', '👍', '👎', '👏', '👐', '👑', '👒', '👓', '👔', '👕', '👖', '👗', '👘', '👙', '👚',
    '👛', '👜', '👝', '👞', '👟', '👠', '👡', '👢', '👣', '👤', '👥', '👦', '👧', '👨', '👩', '👪',
    '👫', '👬', '👭', '👮', '👯', '👰', '👱', '👲', '👳', '👴', '👵', '👶', '👷', '👸', '👹', '👺',
    '👻', '👼', '👽', '👾', '👿', '💀', '💁', '💂', '💃', '💄', '💅', '💆', '💇', '💈', '💉', '💊',
    '💋', '💌', '💍', '💎', '💏', '💐', '💑', '💒', '💓', '💔', '💕', '💖', '💗', '💘', '💙', '💚',
    '💛', '💜', '💝', '💞', '💟', '💠', '💡', '💢', '💣', '💤', '💥', '💦', '💧', '💨', '💩', '💪',
    '💫', '💬', '💭', '💮', '💯', '💰', '💱', '💲', '💳', '💴', '💵', '💶', '💷', '💸', '💹', '💺',
    '💻', '💼', '💽', '💾', '💿', '📀', '📁', '📂', '📃', '📄', '📅', '📆', '📇', '📈', '📉', '📊',
    '📋', '📌', '📍', '📎', '📏', '📐', '📑', '📒', '📓', '📔', '📕', '📖', '📗', '📘', '📙', '📚',
    '📛', '📜', '📝', '📞', '📟', '📠', '📡', '📢', '📣', '📤', '📥', '📦', '📧', '📨', '📩', '📪',
    '📫', '📬', '📭', '📮', '📯', '📰', '📱', '📲', '📳', '📴', '📵', '📶', '📷', '📸', '📹', '📺',
    '📻', '📼', '📿', '🔀', '🔁', '🔂', '🔃', '🔄', '🔅', '🔆', '🔇', '🔈', '🔉', '🔊', '🔋', '🔌',
    '🔍', '🔎', '🔏', '🔐', '🔑', '🔒', '🔓', '🔔', '🔕', '🔖', '🔗', '🔘', '🔙', '🔚', '🔛', '🔜',
    '🔝', '🔞', '🔟', '🔠', '🔡', '🔢', '🔣', '🔤', '🔥', '🔦', '🔧', '🔨', '🔩', '🔪', '🔫', '🔬',
    '🔭', '🔮', '🔯', '🔰', '🔱', '🔲', '🔳', '🔴', '🔵', '🔶', '🔷', '🔸', '🔹', '🔺', '🔻', '🔼',
    '🔽', '🕋', '🕌', '🕍', '🕎', '🕺', '🖕', '🖖', '🖤', '🗻', '🗼', '🗽', '🗾', '🗿', '😀', '😁',
    '😂', '😃', '😄', '😅', '😆', '😇', '😈', '😉', '😊', '😋', '😌', '😍', '😎', '😏', '😐', '😑',
    '😒', '😓', '😔', '😕', '😖', '😗', '😘', '😙', '😚', '😛', '😜', '😝', '😞', '😟', '😠', '😡',
    '😢', '😣', '😤', '😥', '😦', '😧', '😨', '😩', '😪', '😫', '😬', '😭', '😮', '😯', '😰', '😱',
    '😲', '😳', '😴', '😵', '😶', '😷', '😸', '😹', '😺', '😻', '😼', '😽', '😾', '😿', '🙀', '🙁',
    '🙂', '🙃', '🙄', '🙅', '🙆', '🙇', '🙈', '🙉', '🙊', '🙋', '🙌', '🙍', '🙎', '🙏', '🚀', '🚁',
    '🚂', '🚃', '🚄', '🚅', '🚆', '🚇', '🚈', '🚉', '🚊', '🚋', '🚌', '🚍', '🚎', '🚏', '🚐', '🚑',
    '🚒', '🚓', '🚔', '🚕', '🚖', '🚗', '🚘', '🚙', '🚚', '🚛', '🚜', '🚝', '🚞', '🚟', '🚠', '🚡',
    '🚢', '🚣', '🚤', '🚥', '🚦', '🚧', '🚨', '🚩', '🚪', '🚫', '🚬', '🚭', '🚮', '🚯', '🚰', '🚱',
    '🚲', '🚳', '🚴', '🚵', '🚶', '🚷', '🚸', '🚹', '🚺', '🚻', '🚼', '🚽', '🚾', '🚿', '🛀', '🛁',
    '🛂', '🛃', '🛄', '🛅', '🛌', '🛐', '🛑', '🛒', '🛕', '🛫', '🛬', '🛴', '🛵', '🛶', '🛷', '🛸',
    '🛹', '🛺', '🟠', '🟡', '🟢', '🟣', '🟤', '🟥', '🟦', '🟧', '🟨', '🟩', '🟪', '🟫', '🤍', '🤎',
    '🤏', '🤐', '🤑', '🤒', '🤓', '🤔', '🤕', '🤖', '🤗', '🤘', '🤙', '🤚', '🤛', '🤜', '🤝', '🤞',
    '🤟', '🤠', '🤡', '🤢', '🤣', '🤤', '🤥', '🤦', '🤧', '🤨', '🤩', '🤪', '🤫', '🤬', '🤭', '🤮',
    '🤯', '🤰', '🤱', '🤲', '🤳', '🤴', '🤵', '🤶', '🤷', '🤸', '🤹', '🤺', '🤼', '🤽', '🤾', '🤿',
    '🥀', '🥁', '🥂', '🥃', '🥄', '🥅', '🥇', '🥈', '🥉', '🥊', '🥋', '🥌', '🥍', '🥎', '🥏', '🥐',
    '🥑', '🥒', '🥓', '🥔', '🥕', '🥖', '🥗', '🥘', '🥙', '🥚', '🥛', '🥜', '🥝', '🥞', '🥟', '🥠',
    '🥡', '🥢', '🥣', '🥤', '🥥', '🥦', '🥧', '🥨', '🥩', '🥪', '🥫', '🥬', '🥭', '🥮', '🥯', '🥰',
    '🥱', '🥳', '🥴', '🥵', '🥶', '🥺', '🥻', '🥼', '🥽', '🥾', '🥿', '🦀', '🦁', '🦂', '🦃', '🦄',
    '🦅', '🦆', '🦇', '🦈', '🦉', '🦊', '🦋', '🦌', '🦍', '🦎', '🦏', '🦐', '🦑', '🦒', '🦓', '🦔',
    '🦕', '🦖', '🦗', '🦘', '🦙', '🦚', '🦛', '🦜', '🦝', '🦞', '🦟', '🦠', '🦡', '🦢', '🦥', '🦦',
    '🦧', '🦨', '🦩', '🦪', '🦮', '🦯', '🦴', '🦵', '🦶', '🦷', '🦸', '🦹', '🦺', '🦻', '🦼', '🦽',
    '🦾', '🦿', '🧀', '🧁', '🧂', '🧃', '🧄', '🧅', '🧆', '🧇', '🧈', '🧉', '🧊', '🧍', '🧎', '🧏',
    '🧐', '🧑', '🧒', '🧓', '🧔', '🧕', '🧖', '🧗', '🧘', '🧙', '🧚', '🧛', '🧜', '🧝', '🧞', '🧟',
    '🧠', '🧡', '🧢', '🧣', '🧤', '🧥', '🧦', '🧧', '🧨', '🧩', '🧪', '🧫', '🧬', '🧭', '🧮', '🧯',
    '🧰', '🧱', '🧲', '🧳', '🧴', '🧵', '🧶', '🧷', '🧸', '🧹', '🧺', '🧻', '🧼', '🧽', '🧾', '🧿',
    '🩰', '🩱', '🩲', '🩳', '🩸', '🩹', '🩺', '🪀', '🪁', '🪂', '🪐', '🪑', '🪒', '🪓', '🪔', '🪕',
];

/// Why a string is not an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Neither a hex UUID nor thirteen alphabet emoji.
    NotAnId(String),
    /// The emoji form has the wrong number of characters.
    Length { found: usize },
    /// A character outside the alphabet, at this character index.
    NotInAlphabet { index: usize, character: char },
    /// The first digit's index is 256 or more: the value would exceed 128 bits.
    Overflow,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
      Error::NotAnId(s) => write!(
        f,
        "`{s}` is neither a hex UUID (`e1b4bda7-1c7b-4322-b9a0-552201b8a011`) nor an id of {LEN} emoji"
      ),
      Error::Length { found } => write!(f, "an emoji id has {LEN} characters, this one has {found}"),
      Error::NotInAlphabet { index, character } => {
        write!(f, "character {index} ({character:?}) is not in the id alphabet")
      }
      Error::Overflow => write!(f, "the first emoji of an id is one of the first 256 of the alphabet"),
    }
    }
}

impl std::error::Error for Error {}

/// The emoji spelling of `id`.
pub fn encode(id: &Uuid) -> String {
    let mut n = id.as_u128();
    let mut digits = [0usize; LEN];
    for digit in digits.iter_mut().rev() {
        *digit = (n & 0x3ff) as usize;
        n >>= 10;
    }
    digits.iter().map(|&d| ALPHABET[d]).collect()
}

/// The identifier an emoji spelling denotes.
pub fn decode(s: &str) -> Result<Uuid, Error> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() != LEN {
        return Err(Error::Length { found: chars.len() });
    }
    let mut n: u128 = 0;
    for (index, &character) in chars.iter().enumerate() {
        let digit = ALPHABET
            .binary_search(&character)
            .map_err(|_| Error::NotInAlphabet { index, character })?;
        if index == 0 && digit >= 256 {
            return Err(Error::Overflow);
        }
        n = (n << 10) | digit as u128;
    }
    Ok(Uuid::from_u128(n))
}

/// The identifier a string denotes, in either spelling: a hex UUID (with or
/// without hyphens, as [`Uuid::parse_str`] accepts), or thirteen alphabet
/// emoji.
pub fn parse(s: &str) -> Result<Uuid, Error> {
    if let Ok(id) = Uuid::parse_str(s) {
        return Ok(id);
    }
    match decode(s) {
        Ok(id) => Ok(id),
        // A string that is plainly not an attempt at the emoji form gets the
        // message naming both spellings; a near miss keeps its precise cause.
        Err(Error::Length { .. }) if !s.chars().any(|c| ALPHABET.binary_search(&c).is_ok()) => {
            Err(Error::NotAnId(s.to_string()))
        }
        Err(e) => Err(e),
    }
}

/// A fresh random (version 4) identifier.
#[cfg(feature = "v4")]
pub fn v4() -> Uuid {
    Uuid::new_v4()
}

/// Displays an identifier in its emoji spelling.
pub struct Emoji<'a>(pub &'a Uuid);

impl fmt::Display for Emoji<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&encode(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_alphabet_is_sorted_unique_and_free_of_sequences() {
        assert_eq!(ALPHABET.len(), 1024);
        for pair in ALPHABET.windows(2) {
            assert!(
                pair[0] < pair[1],
                "sorted and unique: {:?} before {:?}",
                pair[0],
                pair[1]
            );
        }
        for c in ALPHABET {
            assert!(c as u32 >= 0x2000, "no ASCII or Latin: {c:?}");
            assert!(
                !matches!(c as u32, 0xFE0F | 0x200D | 0x1F3FB..=0x1F3FF),
                "no selector, joiner or skin tone: {c:?}"
            );
            assert!(
                !(0x1F1E6..=0x1F1FF).contains(&(c as u32)),
                "no regional indicator: {c:?}"
            );
            assert!(
                !(0x1F550..=0x1F567).contains(&(c as u32)),
                "no clock face: {c:?}"
            );
            assert!(
                !(0x1F311..=0x1F31A).contains(&(c as u32)),
                "no moon phase: {c:?}"
            );
        }
    }

    #[test]
    fn nil_and_max_pin_the_digit_order() {
        assert_eq!(encode(&Uuid::nil()), ALPHABET[0].to_string().repeat(LEN));
        let max = encode(&Uuid::max());
        let chars: Vec<char> = max.chars().collect();
        assert_eq!(chars[0], ALPHABET[255], "the first digit carries 8 bits");
        assert!(chars[1..].iter().all(|&c| c == ALPHABET[1023]));
        assert_eq!(decode(&max).unwrap(), Uuid::max());
    }

    #[test]
    fn every_identifier_round_trips() {
        for _ in 0..10_000 {
            let id = Uuid::new_v4();
            let emoji = encode(&id);
            assert_eq!(emoji.chars().count(), LEN);
            assert_eq!(decode(&emoji).unwrap(), id, "{emoji}");
            assert_eq!(parse(&emoji).unwrap(), id);
            assert_eq!(parse(&id.to_string()).unwrap(), id);
            assert_eq!(parse(&id.simple().to_string()).unwrap(), id);
        }
    }

    #[test]
    fn a_known_vector() {
        let id = Uuid::parse_str("e1b4bda7-1c7b-4322-b9a0-552201b8a011").unwrap();
        let emoji = encode(&id);
        assert_eq!(decode(&emoji).unwrap(), id);
        assert_eq!(Emoji(&id).to_string(), emoji);
        // The vector, so a change to the alphabet cannot pass unnoticed.
        assert_eq!(emoji, "🎯🚤🧪💲🌼🏪🔘😊🉑🍉⚪🔕♍");
    }

    #[test]
    fn what_is_refused_and_why() {
        assert_eq!(decode("🦊"), Err(Error::Length { found: 1 }));
        let mut too_high = encode(&Uuid::max());
        too_high.replace_range(..ALPHABET[255].len_utf8(), &ALPHABET[256].to_string());
        assert_eq!(decode(&too_high), Err(Error::Overflow));
        let mut foreign = encode(&Uuid::nil());
        foreign.replace_range(..ALPHABET[0].len_utf8(), "a");
        assert_eq!(
            decode(&foreign),
            Err(Error::NotInAlphabet {
                index: 0,
                character: 'a'
            })
        );
        assert_eq!(parse("not an id"), Err(Error::NotAnId("not an id".into())));
        assert!(parse("").is_err());
    }
}
