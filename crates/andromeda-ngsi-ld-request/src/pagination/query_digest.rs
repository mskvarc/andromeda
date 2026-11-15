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
//!
//! A query a request carries in its body has no query string to digest, so it
//! is digested from the parts that hold it, under the same construction.
//!
//! A query that is compiled before its walk is paged is digested from its
//! identity (clause 5.5.9.1): the parameters the compiled query carries are
//! read off its normal form, so `q=a==1;b==2` and `q=b==2;a==1` are one walk,
//! and the parameters it does not carry stay as written.

use crate::query_parameter::QueryParameter;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, str::FromStr};
use thiserror::Error;

/// The query-string parameter a cursor pointer carries the walk's query digest
/// in.
pub const PAGE_QUERY_PARAMETER: &str = "pageQuery";

/// How many leading bytes of the SHA-256 digest a [`QueryDigest`] keeps: enough
/// that two different queries never meet by accident, short enough for a URL.
pub const QUERY_DIGEST_BYTES: usize = 16;

/// The digest of the parameters that decide which elements a walk passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryDigest([u8; QUERY_DIGEST_BYTES]);

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
    /// A parameter a compiled query's identity carries: which entities the
    /// walk passes, read in the normal form of the compiled query rather than
    /// as written.
    Identity,
    /// Which elements the walk passes, or anything not known to be paging or
    /// presentation.
    Selection,
}

impl QueryDigest {
    /// The digest of the selection parameters of `query`, the raw query string
    /// of a request, absent when it has none.
    ///
    /// The parameters are digested decoded and sorted, so a pointer rebuilt
    /// with another percent-encoding or parameter order digests alike. A name
    /// outside the vocabulary counts as selection. This is the digest of a
    /// collection no compiled query selects from: the parameters a compiled
    /// query's identity carries count as selection here, as written.
    #[must_use]
    pub fn of(query: Option<&str>) -> QueryDigest {
        QueryDigest::of_pairs(
            None,
            &selected_pairs(query, |role| matches!(role, ParameterRole::Identity | ParameterRole::Selection)),
        )
    }

    /// The digest of a compiled query: its `identity` record, then the
    /// selection parameters of `query` that the identity does not carry.
    ///
    /// The identity is the first part, so the sequence has an odd number of
    /// parts and never meets the digest of a query string alone
    /// ([`QueryDigest::of`]). The remaining parameters (the temporal window,
    /// `lastN`, the aggregation parameters, the ordering, `datasetId`, `local`
    /// and the rest) are digested decoded and sorted, as written.
    #[must_use]
    pub fn of_identified(identity: &[u8], query: Option<&str>) -> QueryDigest {
        QueryDigest::of_pairs(Some(identity), &selected_pairs(query, |role| role == ParameterRole::Selection))
    }

    /// The digest of `identity`, when there is one, followed by the name and
    /// value of every pair of `pairs`.
    fn of_pairs(identity: Option<&[u8]>, pairs: &[(Cow<'_, str>, Cow<'_, str>)]) -> QueryDigest {
        QueryDigest::of_parts(
            identity
                .into_iter()
                .chain(pairs.iter().flat_map(|(name, value)| [name.as_bytes(), value.as_bytes()])),
        )
    }

    /// The digest of the sequence `parts`.
    ///
    /// Each part is prefixed with its length, so no byte can move across the
    /// boundary between two parts and two different sequences are never hashed
    /// from the same bytes. A query string digests as its name and value pairs,
    /// an even number of parts, so a sequence of an odd number of parts is never
    /// hashed from the bytes of any query string.
    #[must_use]
    pub fn of_parts<'part>(parts: impl IntoIterator<Item = &'part [u8]>) -> QueryDigest {
        let mut hasher = Sha256::new();
        for part in parts {
            hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
            hasher.update(part);
        }
        let digest = hasher.finalize();
        let mut kept = [0; QUERY_DIGEST_BYTES];
        kept.copy_from_slice(&digest[..QUERY_DIGEST_BYTES]);
        QueryDigest(kept)
    }

    /// The token a cursor pointer spells the digest with.
    #[must_use]
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// The digest itself, the bytes [`QueryDigest::encode`] spells.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; QUERY_DIGEST_BYTES] {
        &self.0
    }
}

/// The decoded pairs of `query` whose role `admits`, sorted.
fn selected_pairs(query: Option<&str>, admits: impl Fn(ParameterRole) -> bool) -> Vec<(Cow<'_, str>, Cow<'_, str>)> {
    let mut pairs: Vec<_> = form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .filter(|(name, _)| admits(QueryParameter::from_str(name).map_or(ParameterRole::Selection, role)))
        .collect();
    pairs.sort_unstable();
    pairs
}

/// What `parameter` decides about a paged walk.
const fn role(parameter: QueryParameter) -> ParameterRole {
    match parameter {
        QueryParameter::Limit
        | QueryParameter::Offset
        | QueryParameter::Cursor
        | QueryParameter::PageContext
        | QueryParameter::PageQuery
        | QueryParameter::PostedQuery => ParameterRole::Paging,
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
        | QueryParameter::ScopeQ => ParameterRole::Identity,
        QueryParameter::Timerel
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
pub struct QueryDigestError;

impl TryFrom<&str> for QueryDigest {
    type Error = QueryDigestError;

    fn try_from(text: &str) -> Result<QueryDigest, QueryDigestError> {
        let bytes = URL_SAFE_NO_PAD.decode(text).map_err(|_| QueryDigestError)?;
        <[u8; QUERY_DIGEST_BYTES]>::try_from(bytes.as_slice())
            .map(QueryDigest)
            .map_err(|_| QueryDigestError)
    }
}

#[cfg(test)]
mod tests {
    use crate::pagination::query_digest::{QueryDigest, QueryDigestError};
    use pretty_assertions::{assert_eq, assert_ne};

    fn of(query: &str) -> QueryDigest {
        QueryDigest::of(Some(query))
    }

    #[test]
    fn a_token_reads_back_as_the_digest_it_spells() {
        let digest = of("type=Vehicle&q=speed%3E10");
        assert_eq!(QueryDigest::try_from(digest.encode().as_str()), Ok(digest));
    }

    #[test]
    fn a_token_that_is_not_a_digest_is_refused() {
        assert_eq!(QueryDigest::try_from("%%%"), Err(QueryDigestError));
        assert_eq!(QueryDigest::try_from("YWJj"), Err(QueryDigestError));
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
        assert_eq!(QueryDigest::of(None), of("limit=5&offset=0"));
    }

    /// Unpadded base64url spends one character on every six bits, so the 16
    /// digest bytes take 22.
    #[test]
    fn every_token_spells_the_digest_bytes_in_22_characters() {
        for query in [None, Some("type=Vehicle"), Some("q=speed%3E10&type=Vehicle")] {
            let digest = QueryDigest::of(query);
            assert_eq!(digest.encode().len(), 22);
            assert_eq!(digest.as_bytes().len(), 16);
        }
    }

    #[test]
    fn a_query_string_digests_as_the_sequence_of_its_sorted_pairs() {
        let parts: [&[u8]; 4] = [b"q", b"speed>10", b"type", b"Vehicle"];
        assert_eq!(of("type=Vehicle&q=speed%3E10"), QueryDigest::of_parts(parts));
    }

    #[test]
    fn a_part_boundary_cannot_be_shifted() {
        let shifted: [&[u8]; 2] = [b"ab", b"c"];
        let original: [&[u8]; 2] = [b"a", b"bc"];
        assert_ne!(QueryDigest::of_parts(shifted), QueryDigest::of_parts(original));
    }

    fn identified(query: &str) -> QueryDigest {
        QueryDigest::of_identified(b"identity", Some(query))
    }

    /// QI-3: paging and presentation parameters never decide a walk.
    #[test]
    fn an_identified_digest_ignores_paging_and_presentation_parameters() {
        let walked = identified("timerel=after&timeAt=2020-01-01T00:00:00Z");
        assert_eq!(
            identified("timerel=after&timeAt=2020-01-01T00:00:00Z&limit=3&offset=2&count=true&options=keyValues&pick=speed&omit=brand&lang=en&cursor=YWJj"),
            walked
        );
    }

    /// The identity record carries these parameters in their normal form, so
    /// their written text is not digested a second time.
    #[test]
    fn an_identified_digest_ignores_the_text_of_the_parameters_the_identity_carries() {
        let walked = identified("lastN=2");
        assert_eq!(
            identified("lastN=2&q=b%3D%3D2%3Ba%3D%3D1&type=Vehicle&id=urn:ngsi-ld:Vehicle:1&idPattern=.*&attrs=speed&csf=g%3D%3D1&scopeQ=/a"),
            walked
        );
        assert_eq!(
            identified("lastN=2&georel=near%3BmaxDistance%3D%3D10&geometry=Point&coordinates=%5B0%2C0%5D&geoproperty=location"),
            walked
        );
    }

    #[test]
    fn an_identified_digest_changes_with_the_identity() {
        assert_ne!(
            QueryDigest::of_identified(b"one", Some("lastN=2")),
            QueryDigest::of_identified(b"two", Some("lastN=2"))
        );
    }

    #[test]
    fn an_identified_digest_changes_with_the_parameters_the_identity_does_not_carry() {
        let walked = identified("timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=2&orderBy=id&datasetId=urn:ngsi-ld:Dataset:1");
        for changed in [
            "timerel=after&timeAt=2021-01-01T00:00:00Z&lastN=2&orderBy=id&datasetId=urn:ngsi-ld:Dataset:1",
            "timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=3&orderBy=id&datasetId=urn:ngsi-ld:Dataset:1",
            "timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=2&orderBy=!id&datasetId=urn:ngsi-ld:Dataset:1",
            "timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=2&orderBy=id&datasetId=urn:ngsi-ld:Dataset:2",
        ] {
            assert_ne!(identified(changed), walked, "{changed}");
        }
    }

    /// An identified digest has an odd number of parts, which no query string
    /// digests as.
    #[test]
    fn an_identified_digest_never_meets_the_digest_of_a_query_string() {
        assert_ne!(QueryDigest::of_identified(b"", None), QueryDigest::of(None));
        assert_ne!(QueryDigest::of_identified(b"q", Some("lastN=2")), of("q=lastN&2="));
    }

    #[test]
    fn the_raw_digest_still_counts_the_identity_parameters_as_selection() {
        assert_ne!(of("q=a%3D%3D1%3Bb%3D%3D2"), of("q=b%3D%3D2%3Ba%3D%3D1"));
    }
}
