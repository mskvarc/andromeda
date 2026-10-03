//! A context no single URL names is linked through a hosted wrapper (study
//! B3, B5 to B7, B11 and B17).

use crate::{
    broker_endpoints::broker_url,
    context_api::{JSON_LD_CONTEXT_PREFIX, dereference, extract_id, implicit_context_ids, list_contexts},
    context_links::{context_link, single_context_link},
    core_contexts::{core, core_value},
    independent_processor::IndependentProcessor,
    ngsi_requests::{create_device, get, ok_body, stored_entity},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::{DEVICE_IRI, UserContexts, term_definitions},
};
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Assert `wrapper` is an absolute URL the broker minted for a stored
/// `@context`: clauses 4.14 and 6.3.14 name the tenant in the header alone, so
/// the URL carries no query.
fn assert_minted_wrapper_url(wrapper: &str) {
    assert!(wrapper.starts_with("http://") || wrapper.starts_with("https://"), "absolute: {wrapper}");
    assert!(wrapper.contains(&format!("/ngsi-ld/v1/jsonldContexts/{JSON_LD_CONTEXT_PREFIX}")), "{wrapper}");
    assert!(!wrapper.contains('?'), "the URL names no tenant: {wrapper}");
}

/// POST an `application/ld+json` query naming `context`, answered as
/// `application/json`.
async fn post_query_as_json(client: &Client, device: &str, context: &Value) -> Response {
    client
        .post(broker_url("entityOperations/query"))
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/json")
        .json(&json!({ "type": "Query", "entities": [{ "type": DEVICE_IRI, "id": device }], "@context": context }))
        .send()
        .await
        .expect("POST query failed")
}

/// Study B3: a user context without the core `@context` is linked through one
/// hosted wrapper holding exactly the body's value; a caller sending the
/// tenant's header dereferences it, it is listed `ImplicitlyCreated`, and a
/// processor following the link on the tenant's behalf reads the body as the
/// broker stored it.
#[tokio::test]
async fn a_user_context_without_the_core_is_linked_through_one_hosted_wrapper() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:wrapper");
    create_device(tenant.client(), &device, &contexts.terms).await;

    let response = get(tenant.client(), &format!("entities/{device}"), "application/json", Some(&contexts.terms)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let wrapper = single_context_link(response.headers());
    assert_minted_wrapper_url(&wrapper);
    let mut body: Value = response.json().await.expect("a JSON body");
    assert!(body.get("@context").is_none(), "clause 6.3.6: a json body carries no @context: {body}");
    let dereferenced = dereference(tenant.client(), &wrapper).await;
    assert_eq!(dereferenced.status, StatusCode::OK);
    assert_eq!(dereferenced.json(), json!({ "@context": [contexts.terms, core()] }));
    let (status, implicit) = list_contexts(tenant.client(), "kind=ImplicitlyCreated&details=true").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        implicit
            .iter()
            .any(|entry| entry["kind"] == "ImplicitlyCreated" && entry["localId"] == json!(extract_id(&wrapper))),
        "the wrapper is listed ImplicitlyCreated: {implicit:?}"
    );

    body["@context"] = json!(wrapper);
    let processor = IndependentProcessor::for_tenant(tenant.name()).fetch(&wrapper).await;
    let read = processor.read(&body).await;
    let stored = processor.read_stored(&stored_entity(tenant.client(), &device).await).await;
    assert_eq!(read[0].types, BTreeSet::from([DEVICE_IRI.to_owned()]));
    assert_eq!(read[0].properties, stored.properties, "the linked wrapper reads the body as stored");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study B5: a POST query naming two contexts, or an inline definition, and
/// answered as `application/json`, links one wrapper holding exactly the value
/// an `application/ld+json` body would carry.
#[tokio::test]
async fn a_compound_or_inline_body_context_is_linked_through_a_wrapper_of_the_body_value() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:wrapper-body");
    create_device(tenant.client(), &device, &contexts.terms).await;

    for (context, expected) in [
        (json!([contexts.terms, contexts.importing]), json!([contexts.terms, contexts.importing, core()])),
        (term_definitions(), json!([term_definitions(), core()])),
        (json!([contexts.wrapper, core()]), json!([contexts.wrapper, core()])),
    ] {
        let response = post_query_as_json(tenant.client(), &device, &context).await;
        assert_eq!(response.status(), StatusCode::OK, "{context}");
        let wrapper = single_context_link(response.headers());
        assert_minted_wrapper_url(&wrapper);
        assert_eq!(
            dereference(tenant.client(), &wrapper).await.json(),
            json!({ "@context": expected }),
            "{context}"
        );
    }
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study B6: the same compound request twice links the same wrapper, and the
/// tenant holds one entry for it.
#[tokio::test]
async fn the_same_compound_request_twice_links_one_wrapper() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:wrapper-twice");
    create_device(tenant.client(), &device, &contexts.terms).await;

    let first = single_context_link(
        get(tenant.client(), &format!("entities/{device}"), "application/json", Some(&contexts.terms))
            .await
            .headers(),
    );
    let second = single_context_link(
        get(tenant.client(), &format!("entities/{device}"), "application/json", Some(&contexts.terms))
            .await
            .headers(),
    );
    let from_query = single_context_link(
        get(
            tenant.client(),
            &format!("entities?type=Device&id={device}"),
            "application/json",
            Some(&contexts.terms),
        )
        .await
        .headers(),
    );

    assert_eq!(first, second);
    assert_eq!(first, from_query, "a query under the same context links the same wrapper");
    assert_eq!(implicit_context_ids(tenant.client()).await, vec![extract_id(&first)]);
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study B7: inline definitions differing only in member order and whitespace
/// link the same wrapper; the same URLs in another array order link another.
#[tokio::test]
async fn formatting_does_not_split_a_wrapper_and_array_order_does() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:wrapper-canonical");
    create_device(tenant.client(), &device, &contexts.terms).await;

    let raw_query = |context: &str| format!(r#"{{"type": "Query", "entities": [{{"type": "{DEVICE_IRI}", "id": "{device}"}}], "@context": {context}}}"#);
    let tidy = r#"{"Device": "https://example.org/terms/Device", "speed": "https://example.org/terms/speed"}"#;
    let untidy = "{ \"speed\" :\"https://example.org/terms/speed\",\n\n  \"Device\":\"https://example.org/terms/Device\" }";
    let mut linked = Vec::new();
    for context in [
        tidy.to_owned(),
        untidy.to_owned(),
        format!(r#"["{}", "{}"]"#, contexts.terms, contexts.importing),
        format!(r#"["{}", "{}"]"#, contexts.importing, contexts.terms),
    ] {
        let response = tenant
            .client()
            .post(broker_url("entityOperations/query"))
            .header("Content-Type", "application/ld+json")
            .header("Accept", "application/json")
            .body(raw_query(&context))
            .send()
            .await
            .expect("POST query failed");
        assert_eq!(response.status(), StatusCode::OK, "{context}");
        linked.push(single_context_link(response.headers()));
    }

    assert_eq!(linked[0], linked[1], "member order and whitespace are not content");
    assert_ne!(linked[2], linked[3], "JSON-LD processes an @context array in order");
    assert_eq!(implicit_context_ids(tenant.client()).await.len(), 3);
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study B11: `GeoJSON` with `Prefer: body=json` names its context in the one
/// link, a wrapper for a user context without the core, and none in the body
/// (clause 6.3.6).
#[tokio::test]
async fn geojson_with_prefer_body_json_links_the_wrapper_and_carries_no_body_context() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:wrapper-geojson");
    create_device(tenant.client(), &device, &contexts.terms).await;

    let response = tenant
        .client()
        .get(broker_url(&format!("entities?type=Device&id={device}")))
        .header("Accept", "application/geo+json")
        .header("Prefer", "body=json")
        .header("Link", context_link(&contexts.terms))
        .send()
        .await
        .expect("query failed");

    assert_eq!(response.status(), StatusCode::OK);
    let wrapper = single_context_link(response.headers());
    assert_minted_wrapper_url(&wrapper);
    let body = ok_body(response).await;
    assert!(body.get("@context").is_none(), "no @context in the body: {body}");
    assert!(body["features"][0].get("@context").is_none(), "{body}");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study B17: `/info/sourceIdentity` is answered with the default tenant's
/// loader and store: a json request naming a user context links one wrapper
/// hosted in the default tenant, and an ld+json one names
/// the context followed by the core.
#[tokio::test]
async fn the_source_identity_names_its_context_through_the_default_tenant() {
    let contexts = UserContexts::start().await;
    let client = Client::new();

    let plain = get(&client, "info/sourceIdentity", "application/json", Some(&contexts.terms)).await;
    assert_eq!(plain.status(), StatusCode::OK);
    let wrapper = single_context_link(plain.headers());
    assert!(wrapper.contains(&format!("/ngsi-ld/v1/jsonldContexts/{JSON_LD_CONTEXT_PREFIX}")), "{wrapper}");
    assert!(!wrapper.contains('?'), "the default tenant's URL names no tenant: {wrapper}");
    assert_eq!(dereference(&client, &wrapper).await.json(), json!({ "@context": [contexts.terms, core()] }));

    let ld = ok_body(get(&client, "info/sourceIdentity", "application/ld+json", Some(&contexts.terms)).await).await;
    assert_eq!(ld["@context"], json!([contexts.terms, core_value()]));
    let bare = ok_body(get(&client, "info/sourceIdentity", "application/ld+json", None).await).await;
    assert_eq!(bare["@context"], core_value());
    contexts.shutdown().await;
}
