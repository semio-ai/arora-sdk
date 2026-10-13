use crate::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version(pub [u64; 3]);

impl Version {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let raw = raw.strip_prefix('v').unwrap_or(raw);
        let mut parts = raw.split('.');
        let mut numbers = [0; 3];
        for number in &mut numbers {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            *number = part.parse().ok()?;
        }
        (parts.next().is_none() && numbers >= [1, 0, 0]).then_some(Self(numbers))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

pub(crate) struct Range {
    min: [u128; 3],
    max: Option<[u128; 3]>,
}

impl Range {
    pub fn parse(name: &str, raw: &str) -> Result<Self, Error> {
        let s = raw.trim();
        if s == "*" {
            return Ok(Self {
                min: [0; 3],
                max: None,
            });
        }
        let (prefix, rest) = match s.as_bytes().first() {
            Some(b'^' | b'~') => (&s[..1], &s[1..]),
            _ => ("", s),
        };
        // Range bounds may be below 1.0.0 even though releases cannot be.
        let rest = rest.strip_prefix('v').unwrap_or(rest);
        let parts: Vec<_> = rest.split('.').collect();
        let invalid = || Error::InvalidRange {
            scoped_name: name.into(),
            range: raw.into(),
        };
        if parts.len() != 3 {
            return Err(invalid());
        }
        let mut min = [0u128; 3];
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            min[i] = part.parse::<u64>().map_err(|_| invalid())? as u128;
        }
        let max = match prefix {
            "^" => [min[0] + 1, 0, 0],
            "~" => [min[0], min[1] + 1, 0],
            _ => [min[0], min[1], min[2] + 1],
        };
        Ok(Self {
            min,
            max: Some(max),
        })
    }

    pub fn contains(&self, version: Version) -> bool {
        let v = version.0.map(u128::from);
        v >= self.min && self.max.is_none_or(|max| v < max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_and_ranges() {
        assert!(Version::parse("0.9.9").is_none());
        assert_eq!(Version::parse(" v1.2.3 ").unwrap().to_string(), "1.2.3");
        assert!(Version::parse("1.0.0").is_some());
        assert!(Version::parse("2.10.0") > Version::parse("2.9.0"));
        for bad in [
            "1.2",
            "1.2.3-beta",
            "+1.2.3",
            "18446744073709551616.0.0",
            "1.2.3.4",
        ] {
            assert!(Version::parse(bad).is_none());
            assert!(Range::parse("@test/model", bad).is_err());
        }
        for (range, yes, no) in [
            ("^1.2.3", "1.9.9", "2.0.0"),
            ("~1.2.3", "1.2.9", "1.3.0"),
            ("1.2.3", "1.2.3", "1.2.4"),
        ] {
            let r = Range::parse("@test/model", range).unwrap();
            assert!(r.contains(Version::parse(yes).unwrap()));
            assert!(!r.contains(Version::parse(no).unwrap()));
            assert!(!r.contains(Version::parse("1.2.2").unwrap()));
        }
        for v in [
            "1.0.0",
            "2.10.0",
            "18446744073709551615.18446744073709551615.18446744073709551615",
        ] {
            assert!(Range::parse("@test/model", " * ")
                .unwrap()
                .contains(Version::parse(v).unwrap()));
            assert!(Range::parse("@test/model", v)
                .unwrap()
                .contains(Version::parse(v).unwrap()));
        }
        assert!(Range::parse("@test/model", "^0.9.0").is_ok());
        assert!(Range::parse("@test/model", "^ 1.2.3").is_err());
    }
}
