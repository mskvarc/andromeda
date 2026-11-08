//! The query a cursor walk belongs to (clause 5.5.9.1).
//!
//! A cursor names a position in identifier order and nothing else, so replayed
//! under other filters it would page through another result as if it were the
//! same walk. Every cursor pointer the broker emits therefore carries, beside
//! the cursor, a digest of the request parameters that decide which elements
//! the walk passes, and a request continuing a cursor is refused when its own
//! parameters digest to anything else. Paging parameters, those that only
//! shape how each element or the total is presented, and those that ask for an
//! `EntityMap` of the same query are left out, so a client may follow a pointer
//! with another `limit` or representation, or ask partway through for the
//! result to be mapped.

use crate::query_parameter::QueryParameter;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::str::FromStr;
use thiserror::Error;

/// The query-string parameter a cursor pointer carries the walk's query digest
/// in.
pub const PAGE_QUERY_PARAMETER: &str = "pageQuery";

/// How many leading bytes of the SHA-256 digest the token keeps: enough that
/// two different queries never meet by accident, short enough for a URL.
const DIGEST_BYTES: usize = 16;

/// The digest of the parameters that decide which elements a walk passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageQuery([u8; DIGEST_BYTES]);

/// What a query parameter decides about a paged walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParameterRole {
    /// Where the walk is and how large its pages are.
    Paging,
    /// How each element, or the total, is presented, never which elements the
    /// walk passes.
    Presentation,
    /// An `EntityMap` recorded about the same query (clauses 4.21 and 4.3.6.7):
    /// it lists the entities the query matches and changes none of them. A
    /// map is reused through the `NGSILD-EntityMap` header, never a parameter.
    Artefact,
    /// Which elements the walk passes, or anything not known to be paging or
    /// presentation.
    Selection,
}

impl PageQuery {
    /// The digest of the selection parameters of `query`, the raw query string
    /// of a request, absent when it has none.
    ///
    /// The parameters are digested decoded and sorted, so a pointer rebuilt
    /// with another percent-encoding or parameter order digests alike. A name
    /// outside the vocabulary counts as selection.
    #[must_use]
    pub fn of(query: Option<&str>) -> PageQuery {
        let mut pairs: Vec<_> = form_urlencoded::parse(query.unwrap_or_default().as_bytes())
            .filter(|(name, _)| QueryParameter::from_str(name).map_or(ParameterRole::Selection, role) == ParameterRole::Selection)
            .collect();
        pairs.sort_unstable();
        let pairs = pairs;
        let mut hasher = Sha256::new();
        for (name, value) in &pairs {
            for part in [name.as_bytes(), value.as_bytes()] {
                hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
                hasher.update(part);
            }
        }
        let digest = hasher.finalize();
        let mut kept = [0; DIGEST_BYTES];
        kept.copy_from_slice(&digest[..DIGEST_BYTES]);
        PageQuery(kept)
    }

    /// The token a cursor pointer spells the digest with.
    #[must_use]
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }
}

/// What `parameter` decides about a paged walk.
const fn role(parameter: QueryParameter) -> ParameterRole {
    match parameter {
        QueryParameter::Limit | QueryParameter::Offset | QueryParameter::Cursor | QueryParameter::PageContext | QueryParameter::PageQuery => {
            ParameterRole::Paging
        }
        QueryParameter::Count
        | QueryParameter::Options
        | QueryParameter::Format
        | QueryParameter::Lang
        | QueryParameter::Pick
        | QueryParameter::Omit
        | QueryParameter::GeometryProperty
        | QueryParameter::Join
        | QueryParameter::JoinLevel
        | QueryParameter::Details => ParameterRole::Presentation,
        QueryParameter::EntityMap | QueryParameter::EntityMapLifetime => ParameterRole::Artefact,
        QueryParameter::Id
        | QueryParameter::Type
        | QueryParameter::IdPattern
        | QueryParameter::Attrs
        | QueryParameter::Q
        | QueryParameter::Csf
        | QueryParameter::Georel
        | QueryParameter::Geometry
        | QueryParameter::Coordinates
        | QueryParameter::Geoproperty
        | QueryParameter::ScopeQ
        | QueryParameter::Timerel
        | QueryParameter::TimeAt
        | QueryParameter::EndTimeAt
        | QueryParameter::Timeproperty
        | QueryParameter::Local
        | QueryParameter::OrderBy
        | QueryParameter::Collation
        | QueryParameter::OrderFrom
        | QueryParameter::OrderGeometry
        | QueryParameter::DatasetId
        | QueryParameter::ExpandValues
        | QueryParameter::JsonKeys
        | QueryParameter::ContainedBy
        | QueryParameter::SplitEntities
        | QueryParameter::Drop
        | QueryParameter::Keep
        | QueryParameter::DeleteAll
        | QueryParameter::DryRun
        | QueryParameter::ObservedAt
        | QueryParameter::LastN
        | QueryParameter::AggrMethods
        | QueryParameter::AggrPeriodDuration
        | QueryParameter::AggrGroupBy
        | QueryParameter::Kind
        | QueryParameter::Reload => ParameterRole::Selection,
    }
}

/// A `pageQuery` value that is not a digest the broker emits.
///
/// The value is broker-emitted and followed unchanged, so one that does not
/// decode was corrupted or forged: a `BadRequestData` (clause 6.3.20).
#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid pageQuery: not a query digest")]
pub struct PageQueryError;

impl TryFrom<&str> for PageQuery {
    type Error = PageQueryError;

    fn try_from(text: &str) -> Result<PageQuery, PageQueryError> {
        let bytes = URL_SAFE_NO_PAD.decode(text).map_err(|_| PageQueryError)?;
        <[u8; DIGEST_BYTES]>::try_from(bytes.as_slice()).map(PageQuery).map_err(|_| PageQueryError)
    }
}

#[cfg(test)]
mod tests {
    use crate::pagination::page_query::{PageQuery, PageQueryError};
    use pretty_assertions::{assert_eq, assert_ne};

    fn of(query: &str) -> PageQuery {
        PageQuery::of(Some(query))
    }

    #[test]
    fn a_token_reads_back_as_the_digest_it_spells() {
        let digest = of("type=Vehicle&q=speed%3E10");
        assert_eq!(PageQuery::try_from(digest.encode().as_str()), Ok(digest));
    }

    #[test]
    fn a_token_that_is_not_a_digest_is_refused() {
        assert_eq!(PageQuery::try_from("%%%"), Err(PageQueryError));
        assert_eq!(PageQuery::try_from("YWJj"), Err(PageQueryError));
    }

    #[test]
    fn a_changed_q_type_geo_query_or_identifier_changes_the_digest() {
        let walked = of("type=Vehicle&q=speed%3E10&georel=near%3BmaxDistance%3D%3D10&geometry=Point&coordinates=%5B0%2C0%5D");
        for changed in [
            "type=Vehicle&q=speed%3E11&georel=near%3BmaxDistance%3D%3D10&geometry=Point&coordinates=%5B0%2C0%5D",
            "type=Building&q=speed%3E10&georel=near%3BmaxDistance%3D%3D10&geometry=Point&coordinates=%5B0%2C0%5D",
            "type=Vehicle&q=speed%3E10&georel=near%3BmaxDistance%3D%3D20&geometry=Point&coordinates=%5B0%2C0%5D",
            "type=Vehicle&q=speed%3E10",
            "type=Vehicle&q=speed%3E10&georel=near%3BmaxDistance%3D%3D10&geometry=Point&coordinates=%5B0%2C0%5D&id=urn:ngsi-ld:Vehicle:1",
        ] {
            assert_ne!(of(changed), walked, "{changed}");
        }
    }

    #[test]
    fn paging_and_presentation_parameters_leave_the_digest_unchanged() {
        let walked = of("type=Vehicle&q=speed%3E10");
        assert_eq!(
            of("type=Vehicle&q=speed%3E10&limit=3&cursor=YWJj&pageContext=https%3A%2F%2Fexample.com%2Fa.jsonld&pageQuery=x"),
            walked
        );
        assert_eq!(
            of("type=Vehicle&q=speed%3E10&options=keyValues,sysAttrs&count=true&format=simplified&lang=en&pick=speed&omit=brand&join=flat&joinLevel=2"),
            walked
        );
    }

    /// Clauses 4.21 and 4.3.6.7: an `EntityMap` records the entities the same
    /// query matches, so asking for one partway through a walk selects nothing
    /// else.
    #[test]
    fn asking_for_an_entity_map_leaves_the_digest_unchanged() {
        assert_eq!(
            of("type=Vehicle&q=speed%3E10&entityMap=true&entityMapLifetime=PT1H"),
            of("type=Vehicle&q=speed%3E10")
        );
    }

    #[test]
    fn parameter_order_and_percent_encoding_leave_the_digest_unchanged() {
        assert_eq!(of("q=speed%3E10&type=Vehicle"), of("type=Vehicle&q=speed>10"));
    }

    #[test]
    fn a_name_and_value_boundary_cannot_be_shifted() {
        assert_ne!(of("q=ab"), of("qa=b"));
    }

    #[test]
    fn a_request_without_a_query_digests_like_one_with_only_paging() {
        assert_eq!(PageQuery::of(None), of("limit=5&offset=0"));
    }
}
