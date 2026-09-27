//! Standards-safe construction of federated context-source URLs.

use crate::error::FederationError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
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
pub(crate) fn forwarding_url(endpoint: &Url, path: &[&str], query_string: &str) -> Result<Url, FederationError> {
    let mut url = endpoint.clone(); // clone: a forwarding target owns a mutated URL while the registration remains reusable
    let has_base = ends_with_api_base(endpoint);
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| FederationError::internal(format!("Context source endpoint cannot be a base URL: {endpoint}")))?;
        segments.pop_if_empty();
        if !has_base {
            segments.extend(API_BASE);
        }
        segments.extend(path.iter().copied());
    }
    if !query_string.is_empty() {
        url.query_pairs_mut().extend_pairs(form_urlencoded::parse(query_string.as_bytes()));
    }
    Ok(url)
}

/// Append `local=true` (clause 6.3.18) to a forwarded URL, which is how clause
/// 5.5.13's local scope binds in HTTP when a registration sets `localOnly`.
pub(crate) fn with_local_scope(mut url: Url) -> Url {
    url.query_pairs_mut().append_pair("local", "true");
    url
}

/// Build a forwarding URL for entity queries.
pub(crate) fn build_query_url(endpoint: &Url, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["entities"], query_string)
}

/// Build a forwarding URL for entity retrieval by ID.
pub(crate) fn build_retrieve_url(endpoint: &Url, entity_id: &Uri, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["entities", entity_id.as_str()], query_string)
}

/// Build a forwarding URL for a batch entity query (clause 6.23: POST
/// `/entityOperations/query`).
pub(crate) fn build_batch_query_url(endpoint: &Url) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["entityOperations", "query"], "")
}

/// Build a forwarding URL for discovery queries (types, attributes).
pub(crate) fn build_discovery_url(endpoint: &Url, path: &str, query_string: &str) -> Result<Url, FederationError> {
    let segments = path.split('/').filter(|segment| !segment.is_empty()).collect::<Vec<_>>();
    forwarding_url(endpoint, &segments, query_string)
}

/// Build a forwarding URL for temporal entity queries.
pub(crate) fn build_temporal_query_url(endpoint: &Url, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["temporal", "entities"], query_string)
}

/// Build a forwarding URL for temporal entity retrieval by ID.
pub(crate) fn build_temporal_retrieve_url(endpoint: &Url, entity_id: &Uri, query_string: &str) -> Result<Url, FederationError> {
    forwarding_url(endpoint, &["temporal", "entities", entity_id.as_str()], query_string)
}

#[cfg(test)]
mod tests {
    use crate::forwarding_url::{build_retrieve_url, forwarding_url, with_local_scope};
    use url::Url;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("valid URL")
    }

    #[test]
    fn an_endpoint_without_the_api_base_gets_it_appended() {
        for endpoint in ["http://peer", "http://peer/"] {
            pretty_assertions::assert_eq!(
                forwarding_url(&url(endpoint), &["entities"], "").expect("joins").as_str(),
                "http://peer/ngsi-ld/v1/entities",
                "{endpoint}"
            );
        }
    }

    #[test]
    fn an_endpoint_already_ending_in_the_api_base_is_not_doubled() {
        for endpoint in ["http://peer/ngsi-ld/v1", "http://peer/ngsi-ld/v1/"] {
            pretty_assertions::assert_eq!(
                forwarding_url(&url(endpoint), &["entities"], "").expect("joins").as_str(),
                "http://peer/ngsi-ld/v1/entities",
                "{endpoint}"
            );
        }
        pretty_assertions::assert_eq!(
            forwarding_url(&url("http://peer/prefix/ngsi-ld/v1"), &["entities"], "")
                .expect("joins")
                .as_str(),
            "http://peer/prefix/ngsi-ld/v1/entities"
        );
    }

    #[test]
    fn a_prefixed_endpoint_keeps_its_prefix_below_the_base() {
        pretty_assertions::assert_eq!(
            forwarding_url(&url("http://peer/prefix"), &["entities"], "").expect("joins").as_str(),
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
        let built = forwarding_url(&url("http://peer"), &["entities"], "type=Vehicle").expect("joins");
        pretty_assertions::assert_eq!(with_local_scope(built).as_str(), "http://peer/ngsi-ld/v1/entities?type=Vehicle&local=true");
    }

    #[test]
    fn the_batch_query_url_is_the_entity_operations_query_resource() {
        use crate::forwarding_url::build_batch_query_url;
        for endpoint in ["http://peer", "http://peer/ngsi-ld/v1"] {
            pretty_assertions::assert_eq!(
                build_batch_query_url(&url(endpoint)).expect("joins").as_str(),
                "http://peer/ngsi-ld/v1/entityOperations/query",
                "{endpoint}"
            );
        }
    }
}
