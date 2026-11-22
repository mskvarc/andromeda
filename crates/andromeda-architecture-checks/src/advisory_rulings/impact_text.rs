//! The crate versions an impact statement names, in the two spellings the gate
//! reads.
//!
//! `<name> <version>` names one crate version, such as a crate linking the one
//! ruled on. `<name> <version> or later` names the oldest version of a crate a
//! ruling relies on, such as the first release of a crate that fixes the
//! advisory, so a dependency update that keeps the crate at or above it leaves
//! the text true.

use semver::Version;

/// The words after a crate version that make it the oldest one a ruling relies
/// on.
const FLOOR_SUFFIX: &str = " or later";

/// The oldest version of a crate a ruling relies on, as `<name> <version> or
/// later` spells it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct VersionFloor<'a> {
    /// The crate name.
    pub(crate) name: &'a str,
    /// The version as the text spells it.
    pub(crate) spelt: &'a str,
    /// The version.
    version: Version,
}

impl VersionFloor<'_> {
    /// Whether the locked `version` is at or above the floor; a version that
    /// is no semantic version is not.
    pub(crate) fn admits(&self, version: &str) -> bool {
        Version::parse(version).is_ok_and(|version| version >= self.version)
    }
}

/// Whether `text` names the crate `name` at `version` as the words
/// `<name> <version>`, with neither run on into a longer name or version.
pub(crate) fn names_crate_version(text: &str, name: &str, version: &str) -> bool {
    let words = format!("{name} {version}");
    text.match_indices(&words).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + words.len()..].chars().next();
        !before.is_some_and(is_crate_name_character) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Every `<name> <version> or later` in `text`, in order, whose name can be a
/// crate's and whose version is a semantic version.
pub(crate) fn version_floors(text: &str) -> Vec<VersionFloor<'_>> {
    text.match_indices(FLOOR_SUFFIX)
        .filter(|(start, _)| !text[start + FLOOR_SUFFIX.len()..].starts_with(char::is_alphanumeric))
        .filter_map(|(start, _)| {
            let mut words = text[..start].rsplit(' ');
            let spelt = words.next()?;
            let name = words.next().filter(|name| !name.is_empty() && name.chars().all(is_crate_name_character))?;
            let version = Version::parse(spelt).ok()?;
            Some(VersionFloor { name, spelt, version })
        })
        .collect()
}

/// Whether `character` can be part of a crate name.
fn is_crate_name_character(character: char) -> bool {
    character.is_alphanumeric() || character == '-' || character == '_'
}

#[cfg(test)]
mod tests {
    use crate::advisory_rulings::impact_text::{names_crate_version, version_floors};
    use pretty_assertions::assert_eq;

    fn floors(text: &str) -> Vec<(&str, &str)> {
        version_floors(text).into_iter().map(|floor| (floor.name, floor.spelt)).collect()
    }

    #[test]
    fn a_crate_version_is_named_only_as_whole_words() {
        for (text, named) in [
            ("rumqttc 0.25.1, the only crate linking it", true),
            ("the crate rumqttc 0.25.1.", true),
            ("rumqttc 0.25.1", true),
            ("rumqttc 0.25.10 links it", false),
            ("xrumqttc 0.25.1 links it", false),
            ("old-rumqttc 0.25.1 links it", false),
            ("rumqttc 0.25.2 links it", false),
            ("rumqttc links it", false),
        ] {
            assert_eq!(names_crate_version(text, "rumqttc", "0.25.1"), named, "{text}");
        }
    }

    #[test]
    fn a_crate_version_followed_by_or_later_is_a_floor() {
        assert_eq!(
            floors("the broker verifies every certificate with rustls-webpki 0.103.16 or later, which is patched."),
            [("rustls-webpki", "0.103.16")]
        );
        assert_eq!(floors("rustls-webpki 0.103.16 or later"), [("rustls-webpki", "0.103.16")]);
        assert_eq!(
            floors("rustls 0.23.46 or later and rustls-webpki 0.103.16 or later"),
            [("rustls", "0.23.46"), ("rustls-webpki", "0.103.16")]
        );
    }

    #[test]
    fn words_that_are_no_crate_version_before_or_later_are_no_floor() {
        for text in [
            "rustls-webpki 0.103 or later",
            "five releases or later",
            "rustls-webpki 0.103.16 or latest",
            "rustls-webpki 0.103.16 or laterally",
            "(rustls-webpki 0.103.16 or later)",
            "0.103.16 or later",
            "rustls-webpki 0.103.16",
        ] {
            assert_eq!(floors(text), [] as [(&str, &str); 0], "{text}");
        }
    }

    #[test]
    fn a_floor_admits_its_own_version_and_every_later_one() {
        let floors = version_floors("rustls-webpki 0.103.16 or later");
        let [floor] = floors.as_slice() else {
            panic!("one floor: {floors:?}");
        };
        for (version, admitted) in [
            ("0.103.16", true),
            ("0.103.17", true),
            ("0.104.0", true),
            ("0.103.15", false),
            ("0.102.8", false),
            ("0.103", false),
        ] {
            assert_eq!(floor.admits(version), admitted, "{version}");
        }
    }
}
