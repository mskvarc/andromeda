//! The alias a broker names itself by in a `Via` chain.

use derive_more::Display;
use serde::{Deserialize, Serialize};

/// The alias configured for this broker as a Context Source
/// (`federation.identity.source_alias`, ETSI GS CIM 009 v1.9.1 Table 5.2.40-1).
///
/// The token grammar a `Via` element's `received-by` imposes (RFC 9110 clause
/// 7.6.3) belongs to the lexicon's pseudonym, which also reads every token
/// another broker writes; loading converts the configured alias into it, so a
/// configuration naming an alias outside that grammar is refused there.
#[derive(Debug, Clone, PartialEq, Eq, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(transparent)]
pub struct SourceAlias(String);

impl SourceAlias {
    /// The alias as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// An alias from a fixed literal, for the built-in default.
    #[must_use]
    pub fn from_static(alias: &'static str) -> SourceAlias {
        SourceAlias(alias.to_owned())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::source_alias::SourceAlias;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_alias_round_trips_through_toml_as_its_string() {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Section {
            source_alias: SourceAlias,
        }
        let section: Section = toml::from_str("source_alias = \"broker-a\"").expect("a section");
        assert_eq!(section.source_alias, SourceAlias::from_static("broker-a"));
        assert_eq!(toml::to_string(&section).expect("serialises"), "source_alias = \"broker-a\"\n");
    }
}
