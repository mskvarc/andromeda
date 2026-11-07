//! The request URI as the client addressed it.
//!
//! A problem detail names the request it refuses in its `instance` member, "a
//! URI reference that identifies the specific occurrence of the problem" (IETF
//! RFC 7807 section 3.1, the object ETSI GS CIM 009 v1.9.1 clause 5.5.3 makes
//! every NGSI-LD error body). The
//! broker mounts each API family under a nested router, and nesting strips the
//! mount prefix from the URI its routes and extractors see: inside
//! `/ngsi-ld/v1` a query for entities reads `/entities`, which resolves
//! against the broker to a resource that does not exist. The outermost router
//! records the URI the client sent as an [`OriginalUri`] extension before any
//! nesting rewrites it, and every `instance` an extractor reports is read from
//! there.

use axum::{
    extract::{OriginalUri, Request},
    http::{Extensions, Uri, request::Parts},
};

/// The URI the client addressed, read off a request's parts.
///
/// Falls back to the routed URI only for a request that never passed through a
/// router -- a unit test driving an extractor directly -- where no nesting can
/// have rewritten it.
#[must_use]
pub fn addressed_uri(parts: &Parts) -> Uri {
    original_or_routed(&parts.extensions, &parts.uri)
}

/// The URI the client addressed, read off a whole request, for the extractors
/// and middleware that hold the request rather than its parts.
#[must_use]
pub fn addressed_request_uri<B>(request: &Request<B>) -> Uri {
    original_or_routed(request.extensions(), request.uri())
}

/// The recorded original URI when a router recorded one, else the routed URI.
fn original_or_routed(extensions: &Extensions, routed: &Uri) -> Uri {
    // clone: the problem detail owns its instance, and a `Uri` shares its
    // buffer by reference count.
    extensions.get::<OriginalUri>().map_or(routed, |OriginalUri(original)| original).clone()
}

#[cfg(test)]
mod tests {
    use crate::addressed_uri::{addressed_request_uri, addressed_uri};
    use axum::{
        Router,
        body::{Body, to_bytes},
        extract::{OriginalUri, Request},
        http::{StatusCode, Uri, request::Parts},
        routing::get,
    };
    use tower::ServiceExt;

    /// The URI a handler mounted under a nested router reads through each
    /// accessor, joined so one response carries both.
    async fn both_readings(request: Request) -> String {
        let from_request = addressed_request_uri(&request);
        let (parts, _body) = request.into_parts();
        format!("{from_request} {}", addressed_uri(&parts))
    }

    /// Nesting strips the mount prefix from the routed URI; both accessors
    /// still name the path and query the client sent.
    #[tokio::test]
    async fn a_nested_route_reads_the_uri_the_client_addressed() {
        let router = Router::new().nest("/ngsi-ld/v1", Router::new().route("/entities", get(both_readings)));
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entities?type=Vehicle&q=speed%3E3")
            .body(Body::empty())
            .expect("a valid request");

        let response = router.oneshot(request).await.expect("the router answers");

        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.expect("a readable body");
        pretty_assertions::assert_eq!(
            String::from_utf8_lossy(&body),
            "/ngsi-ld/v1/entities?type=Vehicle&q=speed%3E3 /ngsi-ld/v1/entities?type=Vehicle&q=speed%3E3"
        );
    }

    /// A recorded original URI wins over the routed one.
    #[test]
    fn the_recorded_original_uri_outranks_the_routed_one() {
        let (mut parts, ()) = Request::builder().uri("/entities").body(()).expect("a valid request").into_parts();
        parts.extensions.insert(OriginalUri(Uri::from_static("/ngsi-ld/v1/entities")));

        pretty_assertions::assert_eq!(addressed_uri(&parts), "/ngsi-ld/v1/entities");
    }

    /// Parts no router has seen carry no recorded URI, and nothing has
    /// rewritten theirs.
    #[test]
    fn parts_no_router_has_seen_keep_their_own_uri() {
        let (parts, ()): (Parts, ()) = Request::builder().uri("/ngsi-ld/v1/types").body(()).expect("a valid request").into_parts();

        pretty_assertions::assert_eq!(addressed_uri(&parts), "/ngsi-ld/v1/types");
    }
}
