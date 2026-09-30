//! The validators a served document carries (study E1 to E3, E8, E9).

use crate::{
    context_api::{add_hosted, create_implicit_context, serve, serve_details},
    core_contexts::core,
    ngsi_requests::delete_subscription,
    private_tenant::PrivateTenant,
};
use reqwest::{Method, StatusCode};
use serde_json::json;

/// Whether a `Vary` value names `NGSILD-Tenant`.
fn varies_by_tenant(vary: Option<&str>) -> bool {
    vary.is_some_and(|vary| vary.split(',').any(|name| name.trim().eq_ignore_ascii_case("ngsild-tenant")))
}

/// Study E1: a Hosted document is served exactly as stored, with a positive
/// `max-age`, a strong `ETag` and `Vary: NGSILD-Tenant`.
#[tokio::test]
async fn a_hosted_document_is_served_as_stored_with_max_age_a_strong_tag_and_vary() {
    let tenant = PrivateTenant::provisioned().await;
    let document = json!({ "@context": { "Spot": "https://e.org/Spot" } });
    let id = add_hosted(tenant.client(), &document).await;

    let served = serve(tenant.client(), Method::GET, &id, &[]).await;

    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.json(), document);
    assert!(served.max_age().is_some_and(|seconds| seconds > 0), "{:?}", served.headers);
    let tag = served.header("etag").expect("an entity tag");
    assert!(tag.starts_with('"') && tag.ends_with('"'), "a strong entity tag: {tag}");
    assert!(varies_by_tenant(served.header("vary")), "{:?}", served.headers);
    assert_eq!(served.header("content-type"), Some("application/json"), "Table 6.30.3.1-2 serves a JSON object");
    tenant.finish().await;
}

/// Study E2: an `ImplicitlyCreated` document carries the same validators, and
/// the same `max-age` as a Hosted one, since both are revalidated alike.
#[tokio::test]
async fn an_implicitly_created_document_carries_the_same_validators() {
    let tenant = PrivateTenant::provisioned().await;
    let hosted = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;
    let (subscription_id, implicit) = create_implicit_context(tenant.client()).await;

    let hosted_served = serve(tenant.client(), Method::GET, &hosted, &[]).await;
    let implicit_served = serve(tenant.client(), Method::GET, &implicit, &[]).await;

    assert_eq!(implicit_served.status, StatusCode::OK);
    assert!(implicit_served.header("etag").is_some(), "{:?}", implicit_served.headers);
    assert!(varies_by_tenant(implicit_served.header("vary")), "{:?}", implicit_served.headers);
    assert_eq!(implicit_served.max_age(), hosted_served.max_age());
    delete_subscription(tenant.client(), &subscription_id).await;
    tenant.finish().await;
}

/// Study E3: the tag follows the bytes served: serving a document again
/// repeats it, a different document gets a different one, and the same
/// document stored under another identifier gets the same one.
#[tokio::test]
async fn the_entity_tag_follows_the_document_content() {
    let tenant = PrivateTenant::provisioned().await;
    let spot = json!({ "@context": { "Spot": "https://e.org/Spot" } });
    let first = add_hosted(tenant.client(), &spot).await;
    let changed = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot2" } })).await;
    let twin = add_hosted(tenant.client(), &spot).await;

    let tag = serve(tenant.client(), Method::GET, &first, &[]).await.header("etag").expect("a tag").to_owned();

    assert_eq!(serve(tenant.client(), Method::GET, &first, &[]).await.header("etag"), Some(tag.as_str()));
    assert_ne!(serve(tenant.client(), Method::GET, &changed, &[]).await.header("etag"), Some(tag.as_str()));
    assert_eq!(serve(tenant.client(), Method::GET, &twin, &[]).await.header("etag"), Some(tag.as_str()));
    tenant.finish().await;
}

/// Study E8: the details projection is lifecycle metadata that changes on
/// every access, and carries no validators.
#[tokio::test]
async fn the_details_projection_carries_no_validators() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;

    let details = serve_details(tenant.client(), &id).await;

    assert_eq!(details.status, StatusCode::OK, "{:?}", String::from_utf8_lossy(&details.body));
    assert!(details.header("etag").is_none(), "{:?}", details.headers);
    assert!(details.header("cache-control").is_none(), "{:?}", details.headers);
    tenant.finish().await;
}

/// Study E9: the embedded core `@context` is immutable and carries no
/// validators.
#[tokio::test]
async fn the_embedded_core_context_is_served_without_validators() {
    let tenant = PrivateTenant::provisioned().await;

    let served = serve(tenant.client(), Method::GET, core(), &[]).await;

    assert_eq!(served.status, StatusCode::OK);
    assert!(served.header("etag").is_none(), "{:?}", served.headers);
    assert!(served.header("cache-control").is_none(), "{:?}", served.headers);
    tenant.finish().await;
}
