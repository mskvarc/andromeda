//! The HTTP representation of what the Serve @context operation answers.
//!
//! A `"Hosted"` or `"ImplicitlyCreated"` document (clause 5.13.1) is one a
//! remote broker may cache as a `"Cached"` entry of its own, and clause 6.3.16
//! holds that broker to the `Expires` field and the `max-age` directive the
//! response carries. Without either it falls back to a heuristic lifetime of
//! its own choosing, and keeps serving a document this broker has since
//! deleted for as long as that heuristic says. The response therefore states
//! `max-age`, set to the span after which this broker reads the stored row
//! again itself, and a strong `ETag` (RFC 9110 section 8.8.3) over the exact
//! bytes it sends, so an unchanged document keeps its tag and a changed one
//! does not.
//!
//! Once it sends validators the broker is an origin server that has them, and
//! RFC 7232 section 5 obliges it to evaluate `If-None-Match`: a remote broker
//! revalidating every `max-age` gets 304 without the document. The same path
//! serves a different answer per tenant, errors included, so every Serve
//! response carries `Vary: NGSILD-Tenant` (RFC 9111 section 4.1,
//! [`vary_on_tenant`]), or a shared cache between a client and the broker
//! could hand one tenant's answer to another.

use andromeda_context_registry::served_context::ServedContext;
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_jsonld::loader::cache_lifetime::CacheLifetime;
use axum::{
    Json,
    http::{HeaderValue, StatusCode, header::VARY},
    response::{IntoResponse, Response},
};
use headers::{CacheControl, ContentType, ETag, HeaderMapExt, IfNoneMatch};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The request header a Serve answer is selected by besides its URL.
static VARY_ON_TENANT: HeaderValue = HeaderValue::from_static("NGSILD-Tenant");

/// The response one Serve answer takes, given the request's `If-None-Match`.
///
/// Called only once the tenant, the identifier (clause 5.13.4.4
/// `ResourceNotFound`) and the entry's kind (`OperationNotSupported` for a
/// Cached one) have been resolved, which RFC 7232 section 5 requires before a
/// precondition is evaluated: a request that would not have been a 2xx without
/// it ignores it.
///
/// # Errors
///
/// Returns an internal-error report when a stored document cannot be encoded,
/// which a JSON value read back from the store never is in practice.
pub(crate) fn served_response(served: ServedContext, if_none_match: Option<&IfNoneMatch>) -> Result<Response, ClassifiedReport> {
    match served {
        ServedContext::Metadata(value) | ServedContext::Embedded(value) => Ok(unvalidated_response(value, if_none_match)),
        ServedContext::Stored { document, revalidation } => stored_document_response(&document, revalidation, if_none_match),
    }
}

/// A representation that carries no validator, or 304 when the request's
/// `If-None-Match` is `*`: RFC 7232 section 3.2 makes `*` false whenever a
/// current representation exists, and no entity tag can match one that has
/// none.
fn unvalidated_response(value: Value, if_none_match: Option<&IfNoneMatch>) -> Response {
    if if_none_match == Some(&IfNoneMatch::any()) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (StatusCode::OK, Json(value)).into_response()
    }
}

/// A stored document with the validators a remote cache holds it by, or 304
/// when the request already holds the current one.
fn stored_document_response(document: &Value, revalidation: CacheLifetime, if_none_match: Option<&IfNoneMatch>) -> Result<Response, ClassifiedReport> {
    let body = serde_json::to_vec(document).map_err(|error| ClassifiedReport::internal(&error, InternalFailureSummary::RESPONSE_ENCODING))?;
    let tag = entity_tag(&body).map_err(|error| ClassifiedReport::internal(&error, InternalFailureSummary::RESPONSE_ENCODING))?;
    // RFC 7232 section 3.2: If-None-Match compares weakly, and `*` matches any
    // current representation; a false condition on a GET or HEAD is 304.
    let not_modified = if_none_match.is_some_and(|condition| !condition.precondition_passes(&tag));
    let mut response = if not_modified {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let mut full = (StatusCode::OK, body).into_response();
        full.headers_mut().typed_insert(ContentType::json());
        full
    };
    // RFC 7232 section 4.1: a 304 carries the Cache-Control, ETag and Vary the
    // 200 would have carried.
    let headers = response.headers_mut();
    headers.typed_insert(CacheControl::new().with_max_age(revalidation.as_duration()));
    headers.typed_insert(tag);
    Ok(response)
}

/// Mark a Serve response as selected by the tenant header.
///
/// Layered on the Serve route rather than called per answer, so a 304, a 404
/// for an unknown identifier or tenant and a 422 for a Cached entry carry it
/// as the 200 does: each depends on the tenant the request addressed.
pub(crate) async fn vary_on_tenant(mut response: Response) -> Response {
    response.headers_mut().append(VARY, VARY_ON_TENANT.clone()); // clone: a header value is a cheap shared byte buffer
    response
}

/// The strong entity tag of a representation: the SHA-256 digest of its bytes.
///
/// # Errors
///
/// Returns the header's own parse error, which a quoted hexadecimal digest
/// never produces: every character is one RFC 9110 section 8.8.3 allows in an
/// `etagc`.
fn entity_tag(representation: &[u8]) -> Result<ETag, headers::Error> {
    let digest = Sha256::digest(representation);
    format!("\"{}\"", hex::encode(digest))
        .parse::<ETag>()
        .map_err(|_invalid| headers::Error::invalid())
}

#[cfg(test)]
mod tests {
    use crate::served_document::{served_response, vary_on_tenant};
    use andromeda_context_registry::served_context::ServedContext;
    use andromeda_jsonld::loader::cache_lifetime::CacheLifetime;
    use axum::{
        body::to_bytes,
        http::{
            HeaderName,
            HeaderValue,
            StatusCode,
            header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, VARY},
        },
        response::{IntoResponse, Response},
    };
    use headers::{ETag, IfNoneMatch};
    use serde_json::{Value, json};

    /// A revalidation span the tests can recognise in the header.
    fn revalidation() -> CacheLifetime {
        CacheLifetime::from_seconds(90).expect("ninety seconds is a positive lifetime")
    }

    /// The stored document the tests serve.
    fn spot() -> Value {
        json!({ "@context": { "Spot": "https://e.org/Spot" } })
    }

    /// The response a stored document is served with under `if_none_match`.
    fn served_under(document: Value, if_none_match: Option<&IfNoneMatch>) -> Response {
        served_response(
            ServedContext::Stored {
                document,
                revalidation: revalidation(),
            },
            if_none_match,
        )
        .expect("a stored document is served")
    }

    /// The response a stored document is served with.
    fn served(document: Value) -> Response {
        served_under(document, None)
    }

    /// The value of one header, as text.
    fn header(response: &Response, name: HeaderName) -> Option<String> {
        response.headers().get(name).and_then(|value| value.to_str().ok()).map(str::to_owned)
    }

    /// The condition a request naming `tag` in `If-None-Match` carries.
    fn condition(tag: &str) -> IfNoneMatch {
        IfNoneMatch::from(tag.parse::<ETag>().expect("an entity tag"))
    }

    /// Clause 6.3.16 holds a remote cache to the response's `max-age`, which
    /// is the span after which this broker reads the stored row again itself.
    #[tokio::test]
    async fn a_stored_document_advertises_the_revalidation_span_as_its_max_age() {
        let response = served(spot());

        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        pretty_assertions::assert_eq!(header(&response, CACHE_CONTROL).as_deref(), Some("max-age=90"));
        pretty_assertions::assert_eq!(header(&response, CONTENT_TYPE).as_deref(), Some("application/json"));
    }

    /// RFC 9111 section 4.1: the same path serves a different answer per
    /// tenant header, so the response says so, beside any `Vary` it already
    /// carries.
    #[tokio::test]
    async fn the_tenant_header_joins_the_vary_of_any_answer() {
        let mut compressed = StatusCode::NOT_FOUND.into_response();
        compressed.headers_mut().insert(VARY, HeaderValue::from_static("accept-encoding"));

        let marked = vary_on_tenant(compressed).await;

        let varied: Vec<&str> = marked.headers().get_all(VARY).iter().filter_map(|value| value.to_str().ok()).collect();
        pretty_assertions::assert_eq!(varied, vec!["accept-encoding", "NGSILD-Tenant"]);
        pretty_assertions::assert_eq!(marked.status(), StatusCode::NOT_FOUND);
    }

    /// The tag is a strong validator over the bytes sent: equal documents
    /// share it, and a changed document gets a different one.
    #[tokio::test]
    async fn the_entity_tag_follows_the_document_content() {
        let first = served(spot());
        let same = served(spot());
        let changed = served(json!({ "@context": { "Spot": "https://e.org/Spot2" } }));

        let tag = header(&first, ETAG).expect("an entity tag");
        assert!(tag.starts_with('"') && !tag.starts_with("W/"), "a strong tag: {tag}");
        pretty_assertions::assert_eq!(header(&same, ETAG), Some(tag.clone())); // clone: the tag is compared twice
        assert_ne!(header(&changed, ETAG), Some(tag));
    }

    /// The tag is computed over the bytes the body carries, so the body is the
    /// document exactly as stored.
    #[tokio::test]
    async fn the_body_is_the_stored_document() {
        let document = json!({ "@context": ["https://e.org/a.jsonld", { "b": "https://e.org/b" }] });
        let response = served(document.clone()); // clone: the expectation compares against the input

        let body = to_bytes(response.into_body(), usize::MAX).await.expect("a readable body");
        pretty_assertions::assert_eq!(serde_json::from_slice::<Value>(&body).expect("a JSON body"), document);
    }

    /// RFC 7232 sections 3.2 and 4.1: the current tag in `If-None-Match`
    /// answers 304 with no body and the same `ETag`, `Cache-Control` and
    /// `Vary` a 200 carries.
    #[tokio::test]
    async fn the_current_tag_answers_not_modified_with_the_same_validators() {
        let full = served(spot());
        let tag = header(&full, ETAG).expect("an entity tag");

        let revalidated = served_under(spot(), Some(&condition(&tag)));

        pretty_assertions::assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
        pretty_assertions::assert_eq!(header(&revalidated, ETAG), Some(tag));
        pretty_assertions::assert_eq!(header(&revalidated, CACHE_CONTROL), header(&full, CACHE_CONTROL));
        assert!(header(&revalidated, CONTENT_TYPE).is_none());
        let body = to_bytes(revalidated.into_body(), usize::MAX).await.expect("a readable body");
        assert!(body.is_empty(), "a 304 carries no body: {body:?}");
    }

    /// If-None-Match compares weakly: the weak form of the current tag, which
    /// a compressed response carries, revalidates too.
    #[tokio::test]
    async fn the_weak_form_of_the_current_tag_answers_not_modified() {
        let tag = header(&served(spot()), ETAG).expect("an entity tag");

        let revalidated = served_under(spot(), Some(&condition(&format!("W/{tag}"))));

        pretty_assertions::assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn a_star_answers_not_modified_and_another_tag_answers_the_document() {
        pretty_assertions::assert_eq!(served_under(spot(), Some(&IfNoneMatch::any())).status(), StatusCode::NOT_MODIFIED);
        pretty_assertions::assert_eq!(served_under(spot(), Some(&condition("\"stale\""))).status(), StatusCode::OK);
    }

    /// Metadata changes on every access and an embedded core context is
    /// fixed; neither carries validators.
    #[tokio::test]
    async fn metadata_and_an_embedded_context_carry_no_cache_validators() {
        let metadata = served_response(ServedContext::Metadata(json!({ "localId": "x" })), None).expect("served");
        let embedded = served_response(ServedContext::Embedded(json!({ "@context": {} })), None).expect("served");

        for response in [&metadata, &embedded] {
            pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
            assert!(response.headers().get(ETAG).is_none());
            assert!(response.headers().get(CACHE_CONTROL).is_none());
        }
    }

    /// RFC 7232 section 3.2: `*` is false whenever a current representation
    /// exists, so metadata and an embedded context answer 304 to it; an entity
    /// tag cannot match a representation that has none, so they answer 200 to
    /// one.
    #[tokio::test]
    async fn a_star_revalidates_metadata_and_an_embedded_context_and_a_tag_does_not() {
        let star = IfNoneMatch::any();
        let tag = condition("\"stale\"");
        for served in [
            ServedContext::Metadata(json!({ "localId": "x" })),
            ServedContext::Embedded(json!({ "@context": {} })),
        ] {
            let response = served_response(served, Some(&star)).expect("served");
            pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
            let body = to_bytes(response.into_body(), usize::MAX).await.expect("a readable body");
            assert!(body.is_empty(), "a 304 carries no body: {body:?}");
        }
        for served in [
            ServedContext::Metadata(json!({ "localId": "x" })),
            ServedContext::Embedded(json!({ "@context": {} })),
        ] {
            pretty_assertions::assert_eq!(served_response(served, Some(&tag)).expect("served").status(), StatusCode::OK);
        }
    }
}
