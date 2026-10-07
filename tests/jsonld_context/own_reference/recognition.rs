//! Which references are the tenant's own stored `@context`s.

use crate::{
    broker_endpoints::broker_url,
    context_api::{CONTEXTS_PATH, add_hosted, cached_urls},
    context_links::context_link,
    context_origin::ContextOrigin,
    ngsi_requests::{get, ok_body, stored_entity},
    own_reference::UNREACHABLE_ORIGIN,
    private_tenant::PrivateTenant,
    problem_types::LD_CONTEXT_NOT_AVAILABLE_TYPE,
    unique_ids::unique_id,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

/// Create a `Vehicle` from an `application/json` body read under the one
/// context `link` names: the status and the body.
async fn create_under_link(client: &Client, link: &str) -> (StatusCode, Value, String) {
    let id = unique_id("urn:ngsi-ld:Vehicle:own-reference");
    let response = client
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .header("Link", context_link(link))
        .json(&json!({ "id": id, "type": "Vehicle", "ownSpeed": { "type": "Property", "value": 7 } }))
        .send()
        .await
        .expect("create request failed");
    let status = response.status();
    (status, response.json().await.unwrap_or_default(), id)
}

/// A context mapping `ownSpeed`.
fn own_speed_context() -> Value {
    json!({ "@context": { "ownSpeed": "https://example.org/own/speed" } })
}

/// A root-relative reference to a stored context expands and compacts through
/// the stored document, and the broker records no Cached copy of its own
/// document.
#[tokio::test]
async fn a_root_relative_reference_to_a_stored_context_is_resolved_from_storage() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &own_speed_context()).await;
    let relative = format!("{CONTEXTS_PATH}{id}");

    let (status, body, entity_id) = create_under_link(tenant.client(), &relative).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        stored_entity(tenant.client(), &entity_id).await["https://example.org/own/speed"]["value"],
        json!(7)
    );
    let compacted = ok_body(get(tenant.client(), &format!("entities/{entity_id}"), "application/ld+json", Some(&relative)).await).await;
    assert_eq!(compacted["ownSpeed"]["value"], json!(7), "the stored term compacts the attribute: {compacted}");
    assert!(
        cached_urls(tenant.client()).await.iter().all(|url| !url.contains(CONTEXTS_PATH)),
        "no Cached copy of an own document"
    );
    tenant.finish().await;
}

/// An absolute reference on an origin nothing resolves, used in a tenant's
/// request, is resolved from that tenant's storage: the origin is irrelevant,
/// the reference names no tenant, and the broker never tries to fetch it.
#[tokio::test]
async fn an_absolute_reference_on_any_origin_is_resolved_from_the_requesting_tenants_storage() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &own_speed_context()).await;
    let elsewhere = format!("{UNREACHABLE_ORIGIN}{CONTEXTS_PATH}{id}");

    let (status, body, entity_id) = create_under_link(tenant.client(), &elsewhere).await;

    assert_eq!(status, StatusCode::CREATED, "resolved without a fetch: {body}");
    assert_eq!(
        stored_entity(tenant.client(), &entity_id).await["https://example.org/own/speed"]["value"],
        json!(7)
    );
    assert!(cached_urls(tenant.client()).await.is_empty(), "nothing is recorded as Cached");
    tenant.finish().await;
}

/// Clause 4.14: an entry held only by another tenant is not resolved from that
/// tenant's storage. The reference is read in the requesting tenant's storage,
/// which does not hold it, so it is fetched as the URL it is: on an origin
/// nothing resolves the context is not available, and nothing is recorded as
/// Cached.
#[tokio::test]
async fn an_entry_held_only_by_another_tenant_is_not_resolved_from_it() {
    let owner = PrivateTenant::provisioned().await;
    let other = PrivateTenant::provisioned().await;
    let id = add_hosted(owner.client(), &own_speed_context()).await;
    let owners_entry = format!("{UNREACHABLE_ORIGIN}{CONTEXTS_PATH}{id}");

    let (status, problem, _) = create_under_link(other.client(), &owners_entry).await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{problem}");
    assert_eq!(problem["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str());
    assert!(cached_urls(other.client()).await.is_empty(), "nothing is recorded as Cached");
    owner.finish().await;
    other.finish().await;
}

/// A reference with any query, the vendor tenant parameter included, an extra
/// path segment or another path prefix is not an own reference: it is fetched
/// as the URL it is, and not available on an origin nothing resolves.
#[tokio::test]
async fn an_own_shaped_reference_with_a_query_or_another_path_is_an_ordinary_url() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &own_speed_context()).await;

    for reference in [
        format!("{UNREACHABLE_ORIGIN}{CONTEXTS_PATH}{id}?andromedaTenant={}", tenant.name()),
        format!("{UNREACHABLE_ORIGIN}{CONTEXTS_PATH}{id}?other=1"),
        format!("{UNREACHABLE_ORIGIN}{CONTEXTS_PATH}{id}/extra"),
        format!("{UNREACHABLE_ORIGIN}/elsewhere{CONTEXTS_PATH}{id}"),
    ] {
        let (status, problem, _) = create_under_link(tenant.client(), &reference).await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{reference}: {problem}");
        assert_eq!(problem["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str(), "{reference}");
    }
    tenant.finish().await;
}

/// An own-shaped reference to an identifier the tenant does not hold is
/// fetched like any URL (it may be another broker's document), and is never
/// recorded as Cached, since it is shaped like the broker's own.
#[tokio::test]
async fn an_own_shaped_reference_to_an_unheld_identifier_is_fetched_and_never_recorded() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let foreign_id = "urn:ngsi-ld:JsonLdContext:00000000-0000-4000-8000-000000000001";
    let foreign = origin.serve_document(&format!("{CONTEXTS_PATH}{foreign_id}"), &own_speed_context()).await;

    let (status, body, entity_id) = create_under_link(tenant.client(), &foreign).await;

    assert_eq!(status, StatusCode::CREATED, "the other broker's document is fetched: {body}");
    assert_eq!(
        stored_entity(tenant.client(), &entity_id).await["https://example.org/own/speed"]["value"],
        json!(7)
    );
    assert!(
        !cached_urls(tenant.client()).await.contains(&foreign),
        "an own-shaped URL is never recorded as Cached"
    );
    origin.shutdown().await;
    tenant.finish().await;
}
