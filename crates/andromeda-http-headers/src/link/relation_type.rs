//! Link relation types (RFC 8288 section 2.1).

use andromeda_ngsi_ld_vocabulary::expanded::properties::JSONLD_CONTEXT_REL;
use iri_rs::IriRefBuf;
use std::{fmt, str::FromStr};
use thiserror::Error;

/// A link relation type.
///
/// Only the relation types NGSI-LD actually uses get named variants; every
/// other registered name or extension URI is carried verbatim in
/// [`ExtRelType`](RelationType::ExtRelType).
///
/// Named variants, all from the clause 6.3.10 pagination links:
/// [`Next`](RelationType::Next), [`Prev`](RelationType::Prev),
/// [`First`](RelationType::First), [`Last`](RelationType::Last).
///
/// The JSON-LD context relation (`http://www.w3.org/ns/json-ld#context`) is an
/// extension URI, so it is an `ExtRelType` rather than a variant of its own.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RelationType {
    /// `next` -- next page in a paginated result (clause 6.3.10).
    Next,
    /// `prev` -- previous page in a paginated result (clause 6.3.10).
    Prev,
    /// `first` -- first page in a paginated result (clause 6.3.10).
    First,
    /// `last` -- last page in a paginated result (clause 6.3.10).
    Last,
    /// Any other relation type: a registered name or an extension URI.
    ExtRelType(IriRefBuf),
}

/// An extension relation that is not a valid RFC 3987 IRI reference.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid Link relation type")]
pub struct RelationTypeParseError;

impl RelationType {
    /// The JSON-LD context relation type.
    pub fn jsonld_context() -> RelationType {
        RelationType::ExtRelType(IriRefBuf::from(*JSONLD_CONTEXT_REL.as_iri_ref()))
    }

    /// Whether this is the JSON-LD context relation type.
    pub fn is_jsonld_context(&self) -> bool {
        matches!(self, RelationType::ExtRelType(uri) if uri == JSONLD_CONTEXT_REL.as_str())
    }
}

impl fmt::Display for RelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelationType::Next => f.write_str("next"),
            RelationType::Prev => f.write_str("prev"),
            RelationType::First => f.write_str("first"),
            RelationType::Last => f.write_str("last"),
            RelationType::ExtRelType(uri) => f.write_str(uri),
        }
    }
}

impl FromStr for RelationType {
    type Err = RelationTypeParseError;

    fn from_str(raw: &str) -> Result<RelationType, RelationTypeParseError> {
        Ok(if raw.eq_ignore_ascii_case("next") {
            RelationType::Next
        } else if raw.eq_ignore_ascii_case("prev") {
            RelationType::Prev
        } else if raw.eq_ignore_ascii_case("first") {
            RelationType::First
        } else if raw.eq_ignore_ascii_case("last") {
            RelationType::Last
        } else {
            RelationType::ExtRelType(raw.parse().map_err(|_| RelationTypeParseError)?)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::link::relation_type::RelationType;

    #[test]
    fn each_named_relation_renders_and_parses_back_regardless_of_case() {
        for (variant, token) in [
            (RelationType::Next, "next"),
            (RelationType::Prev, "prev"),
            (RelationType::First, "first"),
            (RelationType::Last, "last"),
        ] {
            pretty_assertions::assert_eq!(variant.to_string(), token);
            pretty_assertions::assert_eq!(token.parse::<RelationType>(), Ok(variant.clone()));
            pretty_assertions::assert_eq!(token.to_uppercase().parse::<RelationType>(), Ok(variant));
        }
    }

    #[test]
    fn an_unnamed_relation_is_carried_verbatim() {
        pretty_assertions::assert_eq!(
            "http://www.w3.org/ns/json-ld#context".parse::<RelationType>(),
            Ok(RelationType::ExtRelType("http://www.w3.org/ns/json-ld#context".parse().expect("IRI reference")))
        );
    }

    #[test]
    fn the_json_ld_context_relation_is_recognised_only_as_itself() {
        pretty_assertions::assert_eq!(
            RelationType::jsonld_context(),
            RelationType::ExtRelType("http://www.w3.org/ns/json-ld#context".parse().expect("IRI reference"))
        );
        assert!(RelationType::jsonld_context().is_jsonld_context());
        assert!(!RelationType::Next.is_jsonld_context());
    }
}
