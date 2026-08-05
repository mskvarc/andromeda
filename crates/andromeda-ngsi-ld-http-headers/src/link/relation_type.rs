//! Link relation types (RFC 8288 section 2.1).

use std::{convert::Infallible, fmt, str::FromStr};

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
    ExtRelType(String),
}

impl RelationType {
    /// The JSON-LD context relation IRI.
    pub const JSONLD_CONTEXT: &str = "http://www.w3.org/ns/json-ld#context";

    /// The JSON-LD context relation type.
    pub fn jsonld_context() -> RelationType {
        RelationType::ExtRelType(Self::JSONLD_CONTEXT.to_owned())
    }

    /// Whether this is the JSON-LD context relation type.
    pub fn is_jsonld_context(&self) -> bool {
        matches!(self, RelationType::ExtRelType(uri) if uri == Self::JSONLD_CONTEXT)
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
    /// Infallible: anything that is not a named relation is an extension URI,
    /// which RFC 8288 permits to be any token.
    type Err = Infallible;

    fn from_str(raw: &str) -> Result<RelationType, Infallible> {
        Ok(if raw.eq_ignore_ascii_case("next") {
            RelationType::Next
        } else if raw.eq_ignore_ascii_case("prev") {
            RelationType::Prev
        } else if raw.eq_ignore_ascii_case("first") {
            RelationType::First
        } else if raw.eq_ignore_ascii_case("last") {
            RelationType::Last
        } else {
            RelationType::ExtRelType(String::from(raw))
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
            Ok(RelationType::ExtRelType("http://www.w3.org/ns/json-ld#context".to_owned()))
        );
    }

    #[test]
    fn the_json_ld_context_relation_is_recognised_only_as_itself() {
        pretty_assertions::assert_eq!(
            RelationType::jsonld_context(),
            RelationType::ExtRelType("http://www.w3.org/ns/json-ld#context".to_owned())
        );
        assert!(RelationType::jsonld_context().is_jsonld_context());
        assert!(!RelationType::Next.is_jsonld_context());
    }
}
