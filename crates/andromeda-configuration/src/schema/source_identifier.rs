//! The IRI a broker names itself by as a Context Source.

use derive_more::{Display, Into};
use iri_rs::{Iri, IriBuf};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// The IRI configured as this broker's Context Source identifier
/// (`federation.identity.source_id`, ETSI GS CIM 009 v1.9.1 Table 5.2.40-1).
///
/// A value of this type is a syntactically valid IRI, so a configuration that
/// names anything else is refused when it loads. The bound an identifier of a
/// stored resource carries is the lexicon's to apply, on the conversion into
/// its identifier.
#[derive(Debug, Clone, PartialEq, Eq, Display, Into, Serialize, Deserialize, JsonSchema)]
#[display("{_0}")]
#[serde(try_from = "String", into = "IriBuf")]
#[schemars(
    with = "String",
    title = "IRI",
    description = "An IRI naming the broker as a context source, such as `urn:ngsi-ld:ContextSourceIdentity:andromeda`."
)]
pub struct SourceIdentifier(IriBuf);

/// The configured `federation.identity.source_id` is not an IRI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("federation.identity.source_id is not an IRI")]
pub struct SourceIdentifierError;

impl SourceIdentifier {
    /// The identifier as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// An identifier from a fixed IRI literal, whose grammar `iri!` checks at
    /// compile time; for the built-in default.
    #[must_use]
    pub fn from_static(iri: Iri<&'static str>) -> SourceIdentifier {
        SourceIdentifier(IriBuf::from(iri))
    }
}

impl FromStr for SourceIdentifier {
    type Err = SourceIdentifierError;

    fn from_str(raw: &str) -> Result<SourceIdentifier, SourceIdentifierError> {
        SourceIdentifier::try_from(raw.to_owned())
    }
}

// `#[serde(try_from = "String")]` needs the owned-string conversion.
impl TryFrom<String> for SourceIdentifier {
    type Error = SourceIdentifierError;

    fn try_from(raw: String) -> Result<SourceIdentifier, SourceIdentifierError> {
        IriBuf::new(raw).map(SourceIdentifier).map_err(|_| SourceIdentifierError)
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::source_identifier::{SourceIdentifier, SourceIdentifierError};
    use iri_rs::iri;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_urn_and_an_http_iri_are_identifiers() {
        for raw in [
            "urn:ngsi-ld:ContextSourceIdentity:andromeda",
            "https://broker.example/identity",
            "urn:ngsi-ld:Ciudad:\u{e9}",
        ] {
            assert_eq!(raw.parse::<SourceIdentifier>().expect("an IRI").as_str(), raw);
        }
    }

    #[test]
    fn a_value_that_is_not_an_iri_is_refused() {
        for raw in ["", "not an iri", "andromeda"] {
            assert_eq!(raw.parse::<SourceIdentifier>(), Err(SourceIdentifierError), "{raw:?}");
        }
    }

    #[test]
    fn the_identifier_round_trips_through_toml_as_its_string() {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Section {
            source_id: SourceIdentifier,
        }
        let section: Section = toml::from_str("source_id = \"urn:ngsi-ld:ContextSourceIdentity:node\"").expect("a section");
        assert_eq!(section.source_id.as_str(), "urn:ngsi-ld:ContextSourceIdentity:node");
        assert_eq!(
            toml::to_string(&section).expect("serialises"),
            "source_id = \"urn:ngsi-ld:ContextSourceIdentity:node\"\n"
        );
        assert!(toml::from_str::<Section>("source_id = \"not an iri\"").is_err());
    }

    #[test]
    fn the_static_constructor_keeps_the_literal() {
        assert_eq!(
            SourceIdentifier::from_static(iri!("urn:ngsi-ld:ContextSourceIdentity:andromeda")).as_str(),
            "urn:ngsi-ld:ContextSourceIdentity:andromeda"
        );
    }
}
