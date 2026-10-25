//! Standards-safe construction of federated context-source URLs.

use crate::{error::FederationError, federation_invariant::FederationInvariant};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use url::{Url, form_urlencoded};

/// The API base every NGSI-LD resource path hangs below (clause 6.2).
const API_BASE: [&str; 2] = ["ngsi-ld", "v1"];

/// Whether a registration endpoint already ends in the API base.
///
/// Clause 5.2.9 describes `endpoint` as the URI "through which the Context
/// Source exposes its NGSI-LD interface", which deployments spell both as the
/// origin and as the origin plus `/ngsi-ld/v1`; appending the base to the
/// latter would double it.
fn ends_with_api_base(endpoint: &Url) -> bool {
    let Some(segments) = endpoint.path_segments() else {
        return false;
    };
    let segments = segments.filter(|segment| !segment.is_empty()).collect::<Vec<_>>();
    segments.len() >= API_BASE.len() && segments[segments.len() - API_BASE.len()..] == API_BASE
}

/// Append API-relative path segments and decoded query pairs to a registration
/// endpoint through [`Url`]'s encoder.
///
/// `path` is the resource path below the API base, so the caller never spells
/// `ngsi-ld/v1` and an endpoint already carrying it is joined without doubling.
/// `query_string` is absent for a request that carries no query; a present one
/// holding no pair adds none either, so no URL ends in a bare `?`.
pub(crate) fn forwarding_url(endpoint: &Url, path: &[&str], query_string: Option<&str>) -> Result<Url, FederationError> {
    let mut url = endpoint.clone(); // clone: a forwarding target owns a mutated URL while the registration remains reusable
    let has_base = ends_with_api_base(endpoint);
    {
        let mut segments = url.path_segments_mut().map_err(|()| {
            FederationError::from(FederationInvariant::EndpointCannotBeBase {
                endpoint: Box::new(endpoint.clone()), // clone: the error owns the endpoint it names
            })
        })?;
        segments.pop_if_empty();
        if !has_base {
            segments.extend(API_BASE);
        }
        segments.extend(path.iter().copied());
    }
    if let Some(query_string) = query_string.filter(|query_string| !query_string.is_empty()) {
        url.query_pairs_mut().extend_pairs(form_urlencoded::parse(query_string.as_bytes()));
    }
    Ok(url)
}

/// The name of the clause 6.3.18 local-scope parameter.
const LOCAL_PARAMETER: &str = "local";

/// The name of the clause 6.3.10 page-start parameter.
const OFFSET_PARAMETER: &str = "offset";

/// Append `local=true` (clause 6.3.18) to a forwarded URL, which is how clause
/// 5.5.13's local scope binds in HTTP when a registration sets `localOnly`.
///
/// A URL already naming the local scope is left as it is: the next-page
/// pointer a source hands back (clause 6.3.10) repeats the parameters of the
/// request that produced it, and a second `local` would be a malformed
/// repetition of a single-valued parameter.
pub(crate) fn with_local_scope(mut url: Url) -> Url {
    if !url.query_pairs().any(|(name, _)| name == LOCAL_PARAMETER) {
        url.query_pairs_mut().append_pair(LOCAL_PARAMETER, "true");
    }
    url
}

/// The same forwarded URL asking for the page that starts at `offset` (clause
/// 6.3.10), replacing an `offset` it already carried.
pub(crate) fn with_offset(url: &Url, offset: u64) -> Url {
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(name, _)| name != OFFSET_PARAMETER)
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    let mut paged = url.clone(); // clone: every page of a walk is its own URL, and the first one stays the walk's origin
    paged
        .query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair(OFFSET_PARAMETER, &offset.to_string());
    paged
}

/// Build a forwarding URL for entity queries.
pub(crate) fn build_query_url(endpoint: &Url, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["entities"], Some(query_string))
}

/// Build a forwarding URL for entity retrieval by ID.
pub(crate) fn build_retrieve_url(endpoint: &Url, entity_id: &Identifier, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["entities", entity_id.as_str()], Some(query_string))
}

/// Build a forwarding URL for a batch entity query (clause 6.23: POST
/// `/entityOperations/query`), carrying the page parameters of `query_string`.
pub(crate) fn build_batch_query_url(endpoint: &Url, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["entityOperations", "query"], Some(query_string))
}

/// Build a forwarding URL for discovery queries (types, attributes), whose
/// path is `segments`: the collection and, when one is addressed, the type
/// or attribute name, which the URL encoder keeps one segment even when it is
/// a full IRI.
pub(crate) fn build_discovery_url(endpoint: &Url, segments: &[&str], query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, segments, Some(query_string))
}

/// Build a forwarding URL for temporal entity queries.
pub(crate) fn build_temporal_query_url(endpoint: &Url, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["temporal", "entities"], Some(query_string))
}

/// Build a forwarding URL for temporal entity retrieval by ID.
pub(crate) fn build_temporal_retrieve_url(endpoint: &Url, entity_id: &Identifier, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["temporal", "entities", entity_id.as_str()], Some(query_string))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::forwarding_url::{build_retrieve_url, forwarding_url, with_local_scope, with_offset};
    use url::Url;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("valid URL")
    }

    #[test]
    fn an_endpoint_without_the_api_base_gets_it_appended() {
        for endpoint in ["http://peer", "http://peer/"] {
            pretty_assertions::assert_eq!(
                forwarding_url(&url(endpoint), &["entities"], None).expect("joins").as_str(),
                "http://peer/ngsi-ld/v1/entities",
                "{endpoint}"
            );
        }
    }

    #[test]
    fn an_endpoint_already_ending_in_the_api_base_is_not_doubled() {
        for endpoint in ["http://peer/ngsi-ld/v1", "http://peer/ngsi-ld/v1/"] {
            pretty_assertions::assert_eq!(
                forwarding_url(&url(endpoint), &["entities"], None).expect("joins").as_str(),
                "http://peer/ngsi-ld/v1/entities",
                "{endpoint}"
            );
        }
        pretty_assertions::assert_eq!(
            forwarding_url(&url("http://peer/prefix/ngsi-ld/v1"), &["entities"], None)
                .expect("joins")
                .as_str(),
            "http://peer/prefix/ngsi-ld/v1/entities"
        );
    }

    #[test]
    fn a_prefixed_endpoint_keeps_its_prefix_below_the_base() {
        pretty_assertions::assert_eq!(
            forwarding_url(&url("http://peer/prefix"), &["entities"], None).expect("joins").as_str(),
            "http://peer/prefix/ngsi-ld/v1/entities"
        );
    }

    /// A `/` and a `%` are URI characters an identifier may carry, but in a
    /// path segment they would split it or be decoded at the peer, so both are
    /// escaped to keep the identifier one segment spelled as the client sent it.
    #[test]
    fn entity_identifiers_and_query_pairs_are_encoded_by_the_url_encoder() {
        let id = "urn:ngsi-ld:Vehicle:A/1%20".parse().expect("valid URI");
        let built = build_retrieve_url(&url("http://peer"), &id, "attrs=speed&type=Vehicle").expect("joins");
        pretty_assertions::assert_eq!(
            built.as_str(),
            "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:A%2F1%2520?attrs=speed&type=Vehicle"
        );
    }

    #[test]
    fn the_local_scope_parameter_is_appended_after_the_forwarded_query() {
        let built = forwarding_url(&url("http://peer"), &["entities"], Some("type=Vehicle")).expect("joins");
        pretty_assertions::assert_eq!(with_local_scope(built).as_str(), "http://peer/ngsi-ld/v1/entities?type=Vehicle&local=true");
    }

    /// A next-page pointer repeats the local scope of the request that produced
    /// it, and the scope is not written a second time.
    #[test]
    fn the_local_scope_is_not_repeated_on_a_url_already_naming_it() {
        let paged = url("http://peer/ngsi-ld/v1/entities?type=Vehicle&local=true&offset=1000");
        pretty_assertions::assert_eq!(with_local_scope(paged).query_pairs().filter(|(name, _)| name == "local").count(), 1);
    }

    /// Clause 6.3.10: a page is requested by its start, replacing the start the
    /// previous page was requested with and keeping every other parameter.
    #[test]
    fn a_page_start_replaces_the_previous_one() {
        let first = url("http://peer/ngsi-ld/v1/entityOperations/query?limit=1000");
        let second = with_offset(&first, 1000);
        pretty_assertions::assert_eq!(second.as_str(), "http://peer/ngsi-ld/v1/entityOperations/query?limit=1000&offset=1000");
        pretty_assertions::assert_eq!(
            with_offset(&second, 2000).as_str(),
            "http://peer/ngsi-ld/v1/entityOperations/query?limit=1000&offset=2000"
        );
    }

    #[test]
    fn the_batch_query_url_is_the_entity_operations_query_resource() {
        use crate::forwarding_url::build_batch_query_url;
        for endpoint in ["http://peer", "http://peer/ngsi-ld/v1"] {
            pretty_assertions::assert_eq!(
                build_batch_query_url(&url(endpoint), "limit=1000").expect("joins").as_str(),
                "http://peer/ngsi-ld/v1/entityOperations/query?limit=1000",
                "{endpoint}"
            );
        }
    }
}
