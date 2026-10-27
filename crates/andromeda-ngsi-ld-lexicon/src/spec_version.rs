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
#[error("NGSI-LD version must be of the form major.minor")]
pub struct SpecVersionParseError;

impl SpecVersion {
    /// NGSI-LD v1.0.
    pub const V1_0: SpecVersion = SpecVersion::new(1, 0);
    /// NGSI-LD v1.3.
    pub const V1_3: SpecVersion = SpecVersion::new(1, 3);
    /// NGSI-LD v1.4.
    pub const V1_4: SpecVersion = SpecVersion::new(1, 4);
    /// NGSI-LD v1.5, whose clause 5.5.8 deletes no member at all.
    pub const V1_5: SpecVersion = SpecVersion::new(1, 5);
    /// NGSI-LD v1.6, which introduces the NGSI-LD Null (clause 4.5.0).
    pub const V1_6: SpecVersion = SpecVersion::new(1, 6);
    /// NGSI-LD v1.8.
    pub const V1_8: SpecVersion = SpecVersion::new(1, 8);
    /// NGSI-LD v1.9.
    pub const V1_9: SpecVersion = SpecVersion::new(1, 9);
    /// The version Andromeda conforms to.
    pub const CONFORMANT: SpecVersion = SpecVersion::V1_9;
    /// The versions Table 4.3.6.8-1 names, oldest first: the points at which
    /// the fallback tables change, and so the only versions a payload can be
    /// said to be shaped for.
    pub const SUPPORTED: [SpecVersion; 5] = [SpecVersion::V1_0, SpecVersion::V1_3, SpecVersion::V1_4, SpecVersion::V1_8, SpecVersion::V1_9];

    /// A version from its two components.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> SpecVersion {
        SpecVersion { major, minor }
    }

    /// The major version component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// The minor version component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }

    /// Whether this version is older than `other`, comparing the major
    /// component first, then the minor: the order the derived `Ord` gives,
    /// spelled so a `const` rule table can read it.
    #[must_use]
    pub const fn precedes(self, other: SpecVersion) -> bool {
        self.major < other.major || (self.major == other.major && self.minor < other.minor)
    }

    /// The closest version in [`SpecVersion::SUPPORTED`] that is not newer
    /// than this one.
    ///
    /// A version the fallback tables do not name (`1.5`, say) is served by the
    /// rules of the nearest named version below it, because clause 4.3.6.8
    /// introduces each fallback at a version and applies it to everything
    /// older. Anything newer than [`SpecVersion::CONFORMANT`] snaps to it, and
    /// anything older than [`SpecVersion::V1_0`] snaps up to `1.0`, the oldest
    /// version the tables describe.
    #[must_use]
    pub fn nearest_supported(self) -> SpecVersion {
        SpecVersion::SUPPORTED
            .iter()
            .rev()
            .copied()
            .find(|supported| *supported <= self)
            .unwrap_or(SpecVersion::V1_0)
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
        let (major, minor) = token.split_once('.').ok_or(SpecVersionParseError)?;
        let major = parse_component(major).ok_or(SpecVersionParseError)?;
        let minor = parse_component(minor).ok_or(SpecVersionParseError)?;
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

    /// `precedes` agrees with the derived order in both directions and is
    /// strict: no version precedes itself.
    #[test]
    fn precedes_is_the_strict_derived_order() {
        let versions = [
            SpecVersion::new(0, 9),
            SpecVersion::V1_0,
            SpecVersion::V1_3,
            SpecVersion::new(1, 5),
            SpecVersion::V1_9,
            SpecVersion::new(1, 10),
            SpecVersion::new(2, 0),
        ];
        for older in versions {
            for newer in versions {
                pretty_assertions::assert_eq!(older.precedes(newer), older < newer, "{older} precedes {newer}");
            }
        }
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
    fn an_unnamed_version_snaps_down_to_the_nearest_supported_one() {
        pretty_assertions::assert_eq!(SpecVersion::new(1, 5).nearest_supported(), SpecVersion::V1_4);
        pretty_assertions::assert_eq!(SpecVersion::new(1, 1).nearest_supported(), SpecVersion::V1_0);
        pretty_assertions::assert_eq!(SpecVersion::new(1, 6).nearest_supported(), SpecVersion::V1_4);
    }

    #[test]
    fn a_named_version_is_its_own_nearest_supported_version() {
        for version in SpecVersion::SUPPORTED {
            pretty_assertions::assert_eq!(version.nearest_supported(), version);
        }
    }

    #[test]
    fn versions_outside_the_supported_range_snap_to_its_ends() {
        pretty_assertions::assert_eq!(SpecVersion::new(2, 0).nearest_supported(), SpecVersion::CONFORMANT);
        pretty_assertions::assert_eq!(SpecVersion::new(1, 12).nearest_supported(), SpecVersion::CONFORMANT);
        pretty_assertions::assert_eq!(SpecVersion::new(0, 9).nearest_supported(), SpecVersion::V1_0);
    }

    #[test]
    fn a_token_outside_the_major_dot_minor_form_does_not_parse() {
        for token in ["v1", "1", "1.5.2", "one.five", "", ".5", "1.", "+1.5", " 1.5"] {
            assert!(SpecVersion::from_str(token).is_err(), "{token:?} must not parse");
        }
    }
}
