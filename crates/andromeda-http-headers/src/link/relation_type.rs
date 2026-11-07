// SPDX-FileCopyrightText: 2014-2025 Sean McArthur
// SPDX-License-Identifier: MIT

//! Link relation types (RFC 8288 section 2.1).
//!
//! This file is derived from the disabled `Link` implementation in
//! <https://github.com/hyperium/headers> and includes local modifications.

use iri_rs::{Iri, IriRefBuf, iri};
use std::{fmt, str::FromStr};
use thiserror::Error;

/// The JSON-LD context relation URI (JSON-LD 1.1 section 3.7).
const JSONLD_CONTEXT_REL: Iri<&'static str> = iri!("http://www.w3.org/ns/json-ld#context");

/// A link relation type.
///
/// Only the relation types NGSI-LD actually uses get named variants; every
/// other registered name or extension URI is carried verbatim in
/// [`ExtRelType`](RelationType::ExtRelType).
///
/// Named variants for pagination links: clause 6.3.10 defines
/// [`Next`](RelationType::Next) and [`Prev`](RelationType::Prev);
/// [`First`](RelationType::First) and [`Last`](RelationType::Last) are
/// additional links Andromeda emits, not defined by that clause.
/// [`Profile`](RelationType::Profile) is the IETF RFC 6906 relation clause
/// 6.3.21 reads an NGSI-LD version profile from.
/// [`Alternate`](RelationType::Alternate) is the RFC 8288 section 6.2.2
/// relation JSON-LD 1.1 section 6.2 names a non-JSON response's JSON-LD
/// alternate by.
///
/// The JSON-LD context relation (`http://www.w3.org/ns/json-ld#context`) is an
/// extension URI, but clause 6.3.5 reads a request's context from it, so it is
/// a named variant, [`JsonLdContext`](RelationType::JsonLdContext), recognised
/// however the client spelled its case (IETF RFC 8288 section 2.1.2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RelationType {
    /// `next` -- next page in a paginated result (clause 6.3.10).
    Next,
    /// `prev` -- previous page in a paginated result (clause 6.3.10).
    Prev,
    /// `first` -- first page in a paginated result.
    First,
    /// `last` -- last page in a paginated result.
    Last,
    /// `profile` -- the target names a profile of the representation (IETF
    /// RFC 6906, clause 6.3.21).
    Profile,
    /// `alternate` -- the target is an alternate representation of the
    /// resource (IETF RFC 8288 section 6.2.2, JSON-LD 1.1 section 6.2).
    Alternate,
    /// `http://www.w3.org/ns/json-ld#context` -- the target is the JSON-LD
    /// context of the document (JSON-LD 1.1 section 6.1, clause 6.3.5).
    JsonLdContext,
    /// Any other relation type: a registered name or an extension URI.
    ExtRelType(IriRefBuf),
}

/// An extension relation that is not a valid RFC 3987 IRI reference.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid Link relation type")]
pub struct RelationTypeParseError;

impl RelationType {
    /// Whether this is the JSON-LD context relation type.
    #[must_use]
    pub const fn is_jsonld_context(&self) -> bool {
        matches!(self, RelationType::JsonLdContext)
    }
}

impl fmt::Display for RelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelationType::Next => f.write_str("next"),
            RelationType::Prev => f.write_str("prev"),
            RelationType::First => f.write_str("first"),
            RelationType::Last => f.write_str("last"),
            RelationType::Profile => f.write_str("profile"),
            RelationType::Alternate => f.write_str("alternate"),
            RelationType::JsonLdContext => f.write_str(JSONLD_CONTEXT_REL.as_str()),
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
        } else if raw.eq_ignore_ascii_case("profile") {
            RelationType::Profile
        } else if raw.eq_ignore_ascii_case("alternate") {
            RelationType::Alternate
        } else if raw.eq_ignore_ascii_case(JSONLD_CONTEXT_REL.as_str()) {
            // IETF RFC 8288 section 2.1.2: an extension relation type is
            // compared "in a case-insensitive fashion, character by character".
            RelationType::JsonLdContext
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
            (RelationType::Profile, "profile"),
            (RelationType::Alternate, "alternate"),
        ] {
            pretty_assertions::assert_eq!(variant.to_string(), token);
            pretty_assertions::assert_eq!(token.parse::<RelationType>(), Ok(variant.clone()));
            pretty_assertions::assert_eq!(token.to_uppercase().parse::<RelationType>(), Ok(variant));
        }
    }

    #[test]
    fn an_unnamed_relation_is_carried_verbatim() {
        pretty_assertions::assert_eq!(
            "https://example.com/rel/describes".parse::<RelationType>(),
            Ok(RelationType::ExtRelType("https://example.com/rel/describes".parse().expect("IRI reference")))
        );
    }

    #[test]
    fn the_json_ld_context_relation_renders_its_uri_and_parses_back() {
        pretty_assertions::assert_eq!(RelationType::JsonLdContext.to_string(), "http://www.w3.org/ns/json-ld#context");
        pretty_assertions::assert_eq!("http://www.w3.org/ns/json-ld#context".parse::<RelationType>(), Ok(RelationType::JsonLdContext));
        assert!(RelationType::JsonLdContext.is_jsonld_context());
        assert!(!RelationType::Next.is_jsonld_context());
    }

    /// IETF RFC 8288 section 2.1.2: extension relation types "MUST be compared
    /// as strings [...] in a case-insensitive fashion", so a client spelling
    /// the JSON-LD context relation in capitals still names the context.
    #[test]
    fn the_json_ld_context_relation_is_recognised_whatever_its_case() {
        for spelling in ["HTTP://WWW.W3.ORG/ns/json-ld#context", "http://www.w3.org/NS/JSON-LD#CONTEXT"] {
            pretty_assertions::assert_eq!(spelling.parse::<RelationType>(), Ok(RelationType::JsonLdContext), "{spelling}");
        }
    }

    /// A relation that merely resembles the JSON-LD context one is not it.
    #[test]
    fn a_relation_that_differs_from_the_json_ld_context_one_is_not_it() {
        let near = "http://www.w3.org/ns/json-ld#contexts".parse::<RelationType>().expect("an extension relation");
        assert!(!near.is_jsonld_context());
    }
}
