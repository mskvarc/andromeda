//! An NGSI-LD specification version in the `major.minor` form (clause 4.3.6.8).

use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// A version of the NGSI-LD specification, as named wherever the API speaks of
/// one: the `Prefer: ngsi-ld=<version>` request preference, its
/// `Preference-Applied` echo, and a registration's `ngsildConformance`
/// `contextSourceInfo` key (clause 4.3.6.6).
///
/// Clause 4.3.6.8 fixes the form to `major.minor`, for example `1.5`, and
/// compares versions by order, so the type is the pair rather than a closed
/// list of the versions Table 4.3.6.8-1 happens to name: a fallback threshold
/// such as "introduced in 1.4" applies to `1.5` as much as to `1.8`. Derived
/// ordering compares the major component first, then the minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
#[display("{major}.{minor}")]
pub struct SpecVersion {
    /// The major version component.
    major: u16,
    /// The minor version component.
    minor: u16,
}

/// Why a token is not a [`SpecVersion`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("NGSI-LD version must be of the form major.minor, got {0}")]
pub struct SpecVersionParseError(String);

impl SpecVersion {
    /// NGSI-LD v1.0.
    pub const V1_0: SpecVersion = SpecVersion::new(1, 0);
    /// NGSI-LD v1.3.
    pub const V1_3: SpecVersion = SpecVersion::new(1, 3);
    /// NGSI-LD v1.4.
    pub const V1_4: SpecVersion = SpecVersion::new(1, 4);
    /// NGSI-LD v1.8.
    pub const V1_8: SpecVersion = SpecVersion::new(1, 8);
    /// NGSI-LD v1.9.
    pub const V1_9: SpecVersion = SpecVersion::new(1, 9);
    /// The version Andromeda conforms to.
    pub const CONFORMANT: SpecVersion = SpecVersion::V1_9;

    /// A version from its two components.
    pub const fn new(major: u16, minor: u16) -> SpecVersion {
        SpecVersion { major, minor }
    }

    /// The major version component.
    pub const fn major(self) -> u16 {
        self.major
    }

    /// The minor version component.
    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// Parse one decimal component, rejecting signs, whitespace and empty text,
/// which `u16::from_str` alone would accept in part.
fn parse_component(component: &str) -> Option<u16> {
    if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    component.parse().ok()
}

// The `major.minor` grammar is two decimal integers around one dot; no derive
// expresses a two-field parse with that shape.
impl FromStr for SpecVersion {
    type Err = SpecVersionParseError;

    fn from_str(token: &str) -> Result<SpecVersion, SpecVersionParseError> {
        let invalid = || SpecVersionParseError(token.to_string());
        let (major, minor) = token.split_once('.').ok_or_else(invalid)?;
        let major = parse_component(major).ok_or_else(invalid)?;
        let minor = parse_component(minor).ok_or_else(invalid)?;
        Ok(SpecVersion { major, minor })
    }
}

#[cfg(test)]
mod tests {
    use crate::spec_version::SpecVersion;
    use std::str::FromStr;

    #[test]
    fn versions_order_by_major_then_minor() {
        assert!(SpecVersion::V1_0 < SpecVersion::V1_3);
        assert!(SpecVersion::V1_3 < SpecVersion::V1_4);
        assert!(SpecVersion::V1_4 < SpecVersion::new(1, 5));
        assert!(SpecVersion::new(1, 5) < SpecVersion::V1_8);
        assert!(SpecVersion::V1_8 < SpecVersion::V1_9);
        assert!(SpecVersion::V1_9 < SpecVersion::new(2, 0));
        assert!(SpecVersion::new(1, 10) > SpecVersion::V1_9);
    }

    #[test]
    fn every_named_version_renders_and_parses_back() {
        for version in [SpecVersion::V1_0, SpecVersion::V1_3, SpecVersion::V1_4, SpecVersion::V1_8, SpecVersion::V1_9] {
            pretty_assertions::assert_eq!(SpecVersion::from_str(&version.to_string()), Ok(version));
        }
        pretty_assertions::assert_eq!(SpecVersion::V1_9.to_string(), "1.9");
    }

    #[test]
    fn a_version_the_tables_do_not_name_still_parses_in_major_dot_minor_form() {
        pretty_assertions::assert_eq!(SpecVersion::from_str("1.5"), Ok(SpecVersion::new(1, 5)));
    }

    #[test]
    fn a_token_outside_the_major_dot_minor_form_does_not_parse() {
        for token in ["v1", "1", "1.5.2", "one.five", "", ".5", "1.", "+1.5", " 1.5"] {
            assert!(SpecVersion::from_str(token).is_err(), "{token:?} must not parse");
        }
    }
}
