//! Standards-safe construction of forwarded context-source URLs.

use crate::error::AndromedaError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use url::{Url, form_urlencoded};

pub(crate) fn build_forwarding_query_string(entity_type: Option<&str>, entity_ids: Option<&[Uri]>) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    if let Some(ids) = entity_ids
        && !ids.is_empty()
    {
        let ids = ids.iter().map(Uri::as_str).collect::<Vec<_>>().join(",");
        serializer.append_pair("id", &ids);
    }
    if let Some(t) = entity_type {
        serializer.append_pair("type", t);
    }
    serializer.finish()
}

/// Append path segments and decoded query pairs through [`Url`]'s encoder.
pub(crate) fn forwarding_url(endpoint: &Url, path: &[&str], query_string: &str) -> Result<Url, AndromedaError> {
    let mut url = endpoint.clone(); // clone: a forwarding target owns a mutated URL while the registration remains reusable
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| AndromedaError::internal(format!("Context source endpoint cannot be a base URL: {endpoint}")))?;
        segments.pop_if_empty();
        segments.extend(path.iter().copied());
    }
    if !query_string.is_empty() {
        url.query_pairs_mut().extend_pairs(form_urlencoded::parse(query_string.as_bytes()));
    }
    Ok(url)
}

/// Build a forwarding URL for entity creation (POST).
pub(crate) fn build_create_url(endpoint: &Url) -> Result<Url, AndromedaError> {
    forwarding_url(endpoint, &["ngsi-ld", "v1", "entities"], "")
}

/// Build a forwarding URL for entity queries.
/// The endpoint may already contain a path (e.g., for redirect mode).
pub(crate) fn build_query_url(endpoint: &Url, query_string: &str) -> Result<Url, AndromedaError> {
    forwarding_url(endpoint, &["ngsi-ld", "v1", "entities"], query_string)
}

/// Build a forwarding URL for entity retrieval by ID.
pub(crate) fn build_retrieve_url(endpoint: &Url, entity_id: &str, query_string: &str) -> Result<Url, AndromedaError> {
    forwarding_url(endpoint, &["ngsi-ld", "v1", "entities", entity_id], query_string)
}

/// Build a forwarding URL for discovery queries (types, attributes).
pub(crate) fn build_discovery_url(endpoint: &Url, path: &str, query_string: &str) -> Result<Url, AndromedaError> {
    let mut segments = vec!["ngsi-ld", "v1"];
    segments.extend(path.split('/').filter(|segment| !segment.is_empty()));
    forwarding_url(endpoint, &segments, query_string)
}

/// Build a forwarding URL for temporal entity queries.
pub(crate) fn build_temporal_query_url(endpoint: &Url, query_string: &str) -> Result<Url, AndromedaError> {
    forwarding_url(endpoint, &["ngsi-ld", "v1", "temporal", "entities"], query_string)
}

/// Build a forwarding URL for temporal entity retrieval by ID.
pub(crate) fn build_temporal_retrieve_url(endpoint: &Url, entity_id: &str, query_string: &str) -> Result<Url, AndromedaError> {
    forwarding_url(endpoint, &["ngsi-ld", "v1", "temporal", "entities", entity_id], query_string)
}
