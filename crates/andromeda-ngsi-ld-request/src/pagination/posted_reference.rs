//! The query string of a followed pointer that names a posted query (clause
//! 5.5.9).
//!
//! A paged query sent by POST is held by the broker, and every pointer of its
//! result is a GET on the query binding of the same operation that names the
//! held query by its digest, beside the URL parameters of the POST and the page
//! asked for. The digest stands for the whole query and the `@context` it was
//! read under (clause 5.5.9.1), so a parameter beside it that the held query
//! already fixes would page over something else: it is refused with
//! `BadRequestData`.

use crate::{
    pagination::query_digest::QueryDigest,
    query_parameter::{QueryParameter, QueryParameterSet},
};
use std::str::FromStr;
use thiserror::Error;

/// The query-string parameter a pointer names a posted query by.
pub const POSTED_QUERY_PARAMETER: &str = "postedQuery";

/// The query string of a followed pointer naming a posted query: its digest, and
/// the URL parameters `P` of the binding that posted it.
#[derive(Debug)]
pub struct PostedQueryReference<P> {
    /// The digest of the posted query the pointer names.
    pub digest: QueryDigest,
    /// The URL parameters of the binding that posted the query, the page
    /// among them.
    pub url: P,
}

// Hand-written because the set is generic over the binding's own set, which
// the declaring macro cannot name.
impl<P: QueryParameterSet> QueryParameterSet for PostedQueryReference<P> {
    fn defines(parameter: QueryParameter) -> bool {
        parameter == QueryParameter::PostedQuery || P::defines(parameter)
    }
}

impl<P: QueryParameterSet> PostedQueryReference<P> {
    /// Whether the raw query string `query` names a posted query.
    #[must_use]
    pub fn is_named_in(query: &str) -> bool {
        form_urlencoded::parse(query.as_bytes()).any(|(name, _value)| name == POSTED_QUERY_PARAMETER)
    }

    /// Refuse a parameter of the raw query string `query` that a pointer
    /// naming a posted query does not carry.
    ///
    /// A name outside the vocabulary is left to the decoding of the query
    /// string, which refuses it as a parameter the operation does not define.
    ///
    /// # Errors
    ///
    /// [`ReselectedParameter`] naming the first parameter of the vocabulary
    /// the set does not define.
    pub fn refuse_reselection(query: &str) -> Result<(), ReselectedParameter> {
        form_urlencoded::parse(query.as_bytes())
            .filter_map(|(name, _value)| QueryParameter::from_str(&name).ok())
            .find(|parameter| !Self::defines(*parameter))
            .map_or(Ok(()), |parameter| Err(ReselectedParameter { parameter }))
    }
}

/// A parameter beside the digest of a posted query that the posted query
/// already fixes.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("{parameter} cannot accompany postedQuery: the posted query it names fixes what the walk selects and how")]
pub struct ReselectedParameter {
    /// The parameter beside the digest.
    pub parameter: QueryParameter,
}

/// A `postedQuery` value that is not the digest of a posted query.
///
/// The value is broker-emitted and followed unchanged, so one that does not
/// decode was corrupted or forged: a `BadRequestData` (clause 6.3.20).
#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid postedQuery: not the digest of a posted query")]
pub struct PostedQueryDigestError;

/// The digest the raw query string `query` names its posted query by, and the
/// rest of the query string: the URL parameters of the binding that posted
/// it, decoded as that binding decodes them.
///
/// # Errors
///
/// [`PostedQueryDigestError`] for a query string naming no digest, more than one, or
/// one that is not a digest the broker emits.
pub fn split_posted_query(query: &str) -> Result<(QueryDigest, String), PostedQueryDigestError> {
    let (named, rest): (Vec<_>, Vec<_>) = form_urlencoded::parse(query.as_bytes()).partition(|(name, _value)| name == POSTED_QUERY_PARAMETER);
    let [(_name, token)] = named.as_slice() else {
        return Err(PostedQueryDigestError);
    };
    let digest = QueryDigest::try_from(token.as_ref()).map_err(|_| PostedQueryDigestError)?;
    Ok((digest, form_urlencoded::Serializer::new(String::new()).extend_pairs(rest).finish()))
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_operations::PostQueryParams,
        pagination::{
            cursor_request::CursorRequest,
            posted_reference::{PostedQueryDigestError, PostedQueryReference, ReselectedParameter, split_posted_query},
            query_digest::QueryDigest,
        },
        query_parameter::QueryParameter,
    };
    use andromeda_ngsi_ld_query::count_policy::CountPolicy;
    use pretty_assertions::assert_eq;

    type EntityReference = PostedQueryReference<PostQueryParams>;

    fn digest() -> QueryDigest {
        QueryDigest::of(Some("type=Vehicle"))
    }

    #[test]
    fn a_pointer_splits_into_its_digest_and_the_post_parameters_it_carries() {
        let (named, rest) = split_posted_query(&format!("limit=2&cursor=&postedQuery={}&count=true", digest().encode())).expect("splits");
        assert_eq!(named, digest());
        let url: PostQueryParams = serde_urlencoded::from_str(&rest).expect("decodes");
        assert_eq!(url.page.limit, Some(2));
        assert_eq!(url.page.cursor, Some(CursorRequest::Start));
        assert_eq!(url.count, CountPolicy::Emit);
    }

    #[test]
    fn only_a_query_string_naming_the_digest_names_a_posted_query() {
        assert!(EntityReference::is_named_in("limit=2&postedQuery=abc"));
        assert!(!EntityReference::is_named_in("type=Vehicle&pageQuery=abc"));
    }

    /// The posted query fixes the selection, the context and the query the
    /// walk is held to, so neither a selection nor a query-string walk's own
    /// pointer parameters may stand beside its digest.
    #[test]
    fn a_parameter_the_posted_query_fixes_is_refused_beside_its_digest() {
        for (query, parameter) in [
            ("postedQuery=k&q=speed%3E1", QueryParameter::Q),
            ("type=Vehicle&postedQuery=k", QueryParameter::Type),
            ("postedQuery=k&pageContext=x", QueryParameter::PageContext),
            ("postedQuery=k&pick=speed", QueryParameter::Pick),
        ] {
            assert_eq!(EntityReference::refuse_reselection(query), Err(ReselectedParameter { parameter }), "{query}");
        }
        assert_eq!(EntityReference::refuse_reselection("postedQuery=k&limit=2&offset=2&options=keyValues"), Ok(()));
        assert_eq!(EntityReference::refuse_reselection("postedQuery=k&notAParameter=1"), Ok(()));
    }

    #[test]
    fn a_key_that_is_no_digest_or_is_named_twice_is_refused() {
        assert_eq!(split_posted_query("postedQuery=abc"), Err(PostedQueryDigestError));
        let token = digest().encode();
        assert_eq!(
            split_posted_query(&format!("postedQuery={token}&postedQuery={token}")),
            Err(PostedQueryDigestError)
        );
    }
}
