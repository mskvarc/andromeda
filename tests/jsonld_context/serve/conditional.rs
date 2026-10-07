//! Conditional Serve @context requests (RFC 7232).
//!
//! Once a document carries an `ETag` the broker is an origin server with
//! validators, and RFC 7232 section 5 obliges it to evaluate `If-None-Match`
//! after its normal checks (tenant, clause 5.13.4.4 `ResourceNotFound`,
//! `OperationNotSupported` for a Cached entry). A false condition answers 304
//! with the `ETag`, `Cache-Control` and `Vary` of the 200 (section 4.1), and the
//! access still counts toward `numberOfHits`: the document "was found in the
//! storage" (clause 5.13.3.5).

use crate::{
    context_api::{add_hosted, find_cached_context_id, number_of_hits, serve, setup_cached_context},
    private_tenant::PrivateTenant,
    problem_types::{OPERATION_NOT_SUPPORTED_TYPE, RESOURCE_NOT_FOUND_TYPE},
};
use reqwest::{Method, StatusCode};
use serde_json::{Map, Value, json};

/// A document well above any compression threshold.
fn large_document() -> Value {
    let terms: Map<String, Value> = (0..200)
        .map(|index| (format!("term{index}"), json!(format!("https://example.org/vocabulary#term{index}"))))
        .collect();
    json!({ "@context": terms })
}

/// The current tag answers 304 with no body and the validators of
/// the 200, and the access is counted.
#[tokio::test]
async fn the_current_entity_tag_answers_not_modified_and_counts_the_access() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;
    let full = serve(tenant.client(), Method::GET, &id, &[]).await;
    assert_eq!(full.status, StatusCode::OK);
    let tag = full.header("etag").expect("an entity tag").to_owned();
    let hits_before = number_of_hits(tenant.client(), &id).await;

    let revalidated = serve(tenant.client(), Method::GET, &id, &[("If-None-Match", &tag)]).await;

    assert_eq!(revalidated.status, StatusCode::NOT_MODIFIED);
    assert!(revalidated.body.is_empty(), "a 304 carries no body");
    for name in ["etag", "cache-control", "vary"] {
        assert_eq!(revalidated.header(name), full.header(name), "{name}");
    }
    assert!(
        number_of_hits(tenant.client(), &id).await > hits_before,
        "a 304 is an access to the stored document"
    );
    tenant.finish().await;
}

/// `If-None-Match` compares weakly, `*` matches the stored
/// document, a list naming the tag among others matches, and a stale tag
/// answers the document.
#[tokio::test]
async fn weak_star_list_and_stale_conditions_are_evaluated_as_rfc_7232_says() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;
    let tag = serve(tenant.client(), Method::GET, &id, &[])
        .await
        .header("etag")
        .expect("an entity tag")
        .to_owned();

    for condition in [format!("W/{tag}"), "*".to_owned(), format!("\"other\", {tag}")] {
        let answered = serve(tenant.client(), Method::GET, &id, &[("If-None-Match", &condition)]).await;
        assert_eq!(answered.status, StatusCode::NOT_MODIFIED, "{condition}");
    }
    let stale = serve(tenant.client(), Method::GET, &id, &[("If-None-Match", "\"stale\"")]).await;
    assert_eq!(stale.status, StatusCode::OK);
    assert_eq!(stale.json(), json!({ "@context": { "Spot": "https://e.org/Spot" } }));
    tenant.finish().await;
}

/// The precondition is evaluated after the normal checks, so an
/// unknown identifier is still 404 and a Cached entry still 422.
#[tokio::test]
async fn a_precondition_does_not_mask_the_normal_refusals() {
    let tenant = PrivateTenant::provisioned().await;
    let (origin, cached_url) = setup_cached_context(tenant.client()).await;
    let cached_id = find_cached_context_id(tenant.client(), &cached_url).await;

    let unknown = serve(tenant.client(), Method::GET, "urn:ngsi-ld:JsonLdContext:unknown", &[("If-None-Match", "*")]).await;
    let cached = serve(tenant.client(), Method::GET, &cached_id, &[("If-None-Match", "*")]).await;

    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    assert_eq!(unknown.json()["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    assert_eq!(cached.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(cached.json()["type"], OPERATION_NOT_SUPPORTED_TYPE.as_str());
    origin.shutdown().await;
    tenant.finish().await;
}

/// A compressed document carries the weak form of its tag (RFC 7232
/// section 2.3.3), and revalidating with it answers 304; the identity
/// representation of the same document keeps the strong tag.
#[tokio::test]
async fn a_compressed_document_carries_a_weak_tag_that_revalidates() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &large_document()).await;

    let compressed = serve(tenant.client(), Method::GET, &id, &[("Accept-Encoding", "gzip")]).await;
    let identity = serve(tenant.client(), Method::GET, &id, &[("Accept-Encoding", "identity")]).await;

    assert_eq!(compressed.status, StatusCode::OK);
    assert_eq!(compressed.header("content-encoding"), Some("gzip"));
    let weak = compressed.header("etag").expect("an entity tag").to_owned();
    assert!(weak.starts_with("W/\""), "an encoded representation carries a weak tag: {weak}");
    let strong = identity.header("etag").expect("an entity tag");
    assert_eq!(format!("W/{strong}"), weak, "the weak tag is the weak form of the identity tag");
    let revalidated = serve(tenant.client(), Method::GET, &id, &[("Accept-Encoding", "gzip"), ("If-None-Match", &weak)]).await;
    assert_eq!(revalidated.status, StatusCode::NOT_MODIFIED);
    tenant.finish().await;
}

/// HEAD answers with the headers of GET and no body, and a
/// conditional HEAD answers 304 alike.
#[tokio::test]
async fn head_answers_with_the_headers_of_get_and_no_body() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;

    let get = serve(tenant.client(), Method::GET, &id, &[]).await;
    let head = serve(tenant.client(), Method::HEAD, &id, &[]).await;

    assert_eq!(head.status, StatusCode::OK);
    assert_eq!(head.body, [] as [u8; 0], "a HEAD answer has no body");
    for name in ["etag", "cache-control", "vary", "content-type"] {
        assert_eq!(head.header(name), get.header(name), "{name}");
    }
    let tag = get.header("etag").expect("an entity tag");
    assert_eq!(
        serve(tenant.client(), Method::HEAD, &id, &[("If-None-Match", tag)]).await.status,
        StatusCode::NOT_MODIFIED
    );
    tenant.finish().await;
}
