//! Shared header logic for NGSI-LD responses.

use crate::{
    domain::ngsi_ld_urn::SnapshotId,
    http::header::{
        link::Link,
        ngsild_entity_map::NgsiLdEntityMap,
        ngsild_snapshot::NgsiLdSnapshot,
        ngsild_warning::{NgsiLdWarning, NgsiLdWarningHeader},
    },
    jsonld::ContextSpec,
    renderer::ContextLocation,
};
use axum::http::{HeaderMap, HeaderValue};
use headers::HeaderMapExt;
use urn::Urn;

/// Options for applying common NGSI-LD headers to a response.
pub struct CommonHeadersOptions<'a> {
    /// `Location` header value for created resources.
    pub location: Option<&'a str>,
    /// NGSILD-EntityMap header URI for entity map scoping (clause 5.7.3.4).
    pub entity_map_uri: Option<&'a str>,
    /// NGSILD-Snapshot header URN for snapshot-scoped responses (clause 6.3.22).
    pub snapshot_uri: Option<&'a SnapshotId>,
    /// NGSILD-Warning header values from distributed forwarding (clause 6.3.17).
    pub warnings: &'a [NgsiLdWarning],
    /// Where the JSON-LD `@context` should appear: in the `Link` header or in the body.
    pub context_location: ContextLocation,
}

/// Apply common NGSI-LD headers to a HeaderMap.
pub fn apply_common_headers(headers: &mut HeaderMap, context: &ContextSpec, options: CommonHeadersOptions<'_>) {
    if let Some(loc) = options.location
        && let Ok(val) = HeaderValue::from_str(loc)
    {
        headers.insert("Location", val);
    }

    if let Some(uri) = options.entity_map_uri
        && let Ok(urn) = uri.parse::<Urn>()
    {
        headers.typed_insert(NgsiLdEntityMap::new(urn));
    }

    if let Some(snap_urn) = options.snapshot_uri {
        headers.typed_insert(NgsiLdSnapshot::new(snap_urn.clone()));
    }

    if !options.warnings.is_empty() {
        headers.typed_insert(NgsiLdWarningHeader::new(options.warnings.to_vec()));
    }

    if options.context_location == ContextLocation::LinkHeader {
        let urls = context.for_presentation().to_link_headers();
        Link::insert_jsonld_contexts(headers, &urls);
    }
}
