//! Shared header logic for NGSI-LD responses.

use andromeda_http_headers::{
    link::header::Link,
    location::Location,
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_snapshot::NgsiLdSnapshot,
    ngsild_warning::{NgsiLdWarning, NgsiLdWarningHeader},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use axum::{http::HeaderMap, response::Response};
use headers::HeaderMapExt;
use iri_rs::IriBuf;

/// Options for applying common NGSI-LD headers to a response.
pub struct CommonHeadersOptions<'a> {
    /// `Location` header of a created resource.
    pub location: Option<&'a Location>,
    /// NGSILD-EntityMap header URI for entity map scoping (clause 5.7.3.4).
    pub entity_map_uri: Option<&'a Identifier>,
    /// NGSILD-Snapshot header URN for snapshot-scoped responses (clause 6.3.22).
    pub snapshot_uri: Option<&'a Identifier>,
    /// NGSILD-Warning header values from distributed forwarding (clause 6.3.17).
    pub warnings: &'a [NgsiLdWarning],
}

/// Apply common NGSI-LD headers to a `HeaderMap`.
///
/// `context_link` is the one context link the rendering resolved, present only
/// when it advertises the `@context` rather than embedding it (clause 6.3.6);
/// it is taken by value, since the response it is written onto is the last
/// thing that reads it.
pub fn apply_common_headers(headers: &mut HeaderMap, context_link: Option<IriBuf>, options: &CommonHeadersOptions<'_>) {
    if let Some(location) = options.location {
        headers.typed_insert(location.clone());
    }

    if let Some(id) = options.entity_map_uri {
        headers.typed_insert(NgsiLdEntityMap::new(id.clone()));
    }

    if let Some(snap_urn) = options.snapshot_uri {
        headers.typed_insert(NgsiLdSnapshot::new(snap_urn.clone()));
    }

    if !options.warnings.is_empty() {
        headers.typed_insert(NgsiLdWarningHeader::new(options.warnings.to_vec()));
    }

    if let Some(target) = context_link {
        Link::insert_jsonld_context(headers, target);
    }
}

/// Attach a `Location` header to an existing response.
#[must_use]
pub fn with_location(mut response: Response, location: Location) -> Response {
    response.headers_mut().typed_insert(location);
    response
}

/// Attach the `NGSILD-EntityMap` header to an existing response.
///
/// Table 6.4.3.3-2: "The location of the `EntityMap` used in the purge
/// operation is returned in the response." A purge answers 204 or 207 with no
/// rendered body, so its map location joins a finished response here rather
/// than travelling through the rendering pipeline [`NgsiLdResponse`] serves.
///
/// [`NgsiLdResponse`]: crate::single::NgsiLdResponse
#[must_use]
pub fn with_entity_map(mut response: Response, identifier: &Identifier) -> Response {
    response.headers_mut().typed_insert(NgsiLdEntityMap::new(identifier.clone()));
    response
}

#[cfg(test)]
mod tests {
    use crate::headers::{with_entity_map, with_location};
    use andromeda_http_headers::location::Location;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use axum::{
        http::StatusCode,
        response::{IntoResponse, Response},
    };

    /// A finished response with no headers of its own.
    fn no_content() -> Response {
        StatusCode::NO_CONTENT.into_response()
    }

    /// The value of one header, as wire text.
    fn header_text<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
        response.headers().get(name).and_then(|value| value.to_str().ok())
    }

    /// Table 6.4.3.3-2: the location of the map an operation used travels back
    /// on a response that carries no rendered body.
    #[test]
    fn the_entity_map_location_joins_a_finished_response() {
        let identifier: Identifier = "urn:ngsi-ld:EntityMap:used".parse().expect("valid URI");
        let response = with_entity_map(no_content(), &identifier);
        pretty_assertions::assert_eq!(response.status(), StatusCode::NO_CONTENT, "attaching a header does not change the status");
        pretty_assertions::assert_eq!(header_text(&response, "ngsild-entitymap"), Some("urn:ngsi-ld:EntityMap:used"));
    }

    /// The two attachments are independent, so a response may carry either or
    /// both without one displacing the other.
    #[test]
    fn a_location_and_an_entity_map_location_do_not_displace_each_other() {
        let identifier: Identifier = "urn:ngsi-ld:EntityMap:used".parse().expect("valid URI");
        let location = Location::new("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1").expect("a valid location");
        let response = with_entity_map(with_location(no_content(), location), &identifier);
        pretty_assertions::assert_eq!(header_text(&response, "location"), Some("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1"));
        pretty_assertions::assert_eq!(header_text(&response, "ngsild-entitymap"), Some("urn:ngsi-ld:EntityMap:used"));
    }
}
