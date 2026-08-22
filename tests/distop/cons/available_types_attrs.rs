//! System-wide available types and attributes (clauses 5.7.5–5.7.11): a
//! registration makes a type/attribute possibly-available even with no local data,
//! detail arrays and counts may be empty or estimates (`MAY` rows assert
//! *permitted*, never *required*), and `/types` may be answered from the local
//! datastore and Context Registry alone.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use axum::http::Method;
use serde_json::{Value, json};

/// A JSON-LD Link header value carrying the compound test context.
fn link_value() -> String {
    format!(
        "<{}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
        COMPOUND_CONTEXT.as_str()
    )
}

/// Whether a JSON string array contains the term, allowing an expanded-IRI form.
fn contains_named(list: &Value, name: &str) -> bool {
    list.as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item.as_str()
                .is_some_and(|value| value == name || value.ends_with(&format!("/{name}")) || value.ends_with(&format!("#{name}")))
        })
    })
}

/// A GET against a details resource on the front broker, carrying the context.
async fn get_detail(topology: &andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>, path: &str) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, path)
        .header("accept", "application/ld+json")
        .header("link", link_value().as_str())
        .send()
        .await
        .expect("detail request")
}

/// A registration for a type not held locally makes that type appear system-wide in
/// `/types` as possibly-available (clauses 5.7.5, 5.7.6). Confirms REQ-CONS-74,
/// REQ-CONS-81.
#[tokio::test]
async fn distop_available_types_reflects_registrations_system_wide() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    let registration_id = topology.id("registration:cons74");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, peer.peer_url())
                    .mode("inclusive")
                    .entity_type("Aircraft")
                    .operations(&["retrieveEntity", "queryEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );

    let response = front.client().list_types("", Some(COMPOUND_CONTEXT.as_str())).await.expect("types");
    assert_status(&response, 200);
    assert!(
        contains_named(&json_body(&response)["typeList"], "Aircraft"),
        "the registered type appears system-wide: {}",
        json_body(&response)
    );

    topology.finish().await.expect("topology cleanup");
}

/// System-wide type details are returned, and `attributeDetails`/`attributeNames`
/// may be an empty array when the registration lacks that detail (clauses 5.7.7,
/// 5.7.6). Confirms REQ-CONS-75, REQ-CONS-82.
#[tokio::test]
async fn distop_entity_type_details_system_wide_attribute_details_may_be_empty() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    let registration_id = topology.id("registration:cons75");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, peer.peer_url())
                    .mode("inclusive")
                    .entity_type("Aircraft")
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );

    let response = get_detail(&topology, "/ngsi-ld/v1/types/Aircraft").await;
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(body.get("typeName").is_some(), "type details are returned system-wide: {body}");
    assert!(
        body.get("attributeDetails").is_none_or(|details| details.is_array()),
        "attributeDetails may be present and empty: {body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// `/types/{type}` returns an `entityCount`; the value may be an estimate over the
/// distributed sources (clauses 5.7.7). Confirms REQ-CONS-76, REQ-CONS-83.
#[tokio::test]
async fn distop_entity_type_info_count_may_be_estimate() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    for index in 0..2 {
        let aircraft = entity(
            &topology.id(&format!("Aircraft:cons76-{index}")),
            "Aircraft",
            &[("thrust", property(json!(1000)))],
        );
        assert_status(&peer.client().post_entity(&aircraft).with_ld_json().send().await.expect("peer aircraft"), 201);
    }
    let registration_id = topology.id("registration:cons76");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, peer.peer_url())
                    .mode("inclusive")
                    .entity_type("Aircraft")
                    .operations(&["retrieveEntity", "queryEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );

    let response = get_detail(&topology, "/ngsi-ld/v1/types/Aircraft").await;
    assert_status(&response, 200);
    assert!(
        json_body(&response)["entityCount"].as_i64().is_none_or(|count| count >= 0),
        "entityCount is a non-negative estimate: {}",
        json_body(&response)
    );

    topology.finish().await.expect("topology cleanup");
}

/// A registration's `propertyNames` makes an attribute appear system-wide in
/// `/attributes` even with no local data (clauses 5.7.9). Confirms REQ-CONS-77,
/// REQ-CONS-81.
#[tokio::test]
async fn distop_available_attributes_system_wide() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    let registration_id = topology.id("registration:cons77");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, peer.peer_url())
                    .mode("inclusive")
                    .entity_type("Aircraft")
                    .property_names(&["thrust"])
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );

    let response = front.client().list_attributes("", Some(COMPOUND_CONTEXT.as_str())).await.expect("attributes");
    assert_status(&response, 200);
    assert!(
        contains_named(&json_body(&response)["attributeList"], "thrust"),
        "the registered attribute appears system-wide: {}",
        json_body(&response)
    );

    topology.finish().await.expect("topology cleanup");
}

/// System-wide attribute details are returned, and `typeNames` may be an empty array
/// when the registration lacks type detail (clauses 5.7.11). Confirms REQ-CONS-78,
/// REQ-CONS-82.
#[tokio::test]
async fn distop_attribute_details_system_wide_may_be_empty() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    let registration_id = topology.id("registration:cons78");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, peer.peer_url())
                    .mode("inclusive")
                    .without_entity()
                    .property_names(&["thrust"])
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );

    let response = get_detail(&topology, "/ngsi-ld/v1/attributes/thrust").await;
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(body.get("attributeName").is_some(), "attribute details are returned system-wide: {body}");
    assert!(
        body.get("typeNames").is_none_or(|names| names.is_array()),
        "typeNames may be present and empty: {body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// `/attributes/{attr}` returns counts; the values may be estimates (clauses 5.7.11).
/// Confirms REQ-CONS-79, REQ-CONS-83.
#[tokio::test]
async fn distop_attribute_info_count_may_be_estimate() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    let aircraft = entity(&topology.id("Aircraft:cons79"), "Aircraft", &[("thrust", property(json!(1000)))]);
    assert_status(&peer.client().post_entity(&aircraft).with_ld_json().send().await.expect("peer aircraft"), 201);
    let registration_id = topology.id("registration:cons79");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, peer.peer_url())
                    .mode("inclusive")
                    .entity_type("Aircraft")
                    .property_names(&["thrust"])
                    .operations(&["retrieveEntity", "queryEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );

    let response = get_detail(&topology, "/ngsi-ld/v1/attributes/thrust").await;
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(
        body["attributeCount"].as_i64().is_none_or(|count| count >= 0),
        "attributeCount is a non-negative estimate: {body}"
    );
    assert!(
        body["entityCount"].as_i64().is_none_or(|count| count >= 0),
        "entityCount is a non-negative estimate: {body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// `/types` lists a registered type; whether the sources are contacted is a `MAY`,
/// so the type must be present and the source-contact is only a classifying
/// observation (clauses 5.7.5). Confirms REQ-CONS-80.
#[tokio::test]
async fn distop_available_types_answered_from_local_and_registry_without_contacting_sources() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons80");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_type("Aircraft")
                    .operations(&["retrieveEntity", "queryEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        "/ngsi-ld/v1/types",
        200,
        &json!({ "id": "urn:ngsi-ld:EntityTypeList:mock", "type": "EntityTypeList", "typeList": ["Aircraft"] }),
    )
    .await;

    let response = front.client().list_types("", Some(COMPOUND_CONTEXT.as_str())).await.expect("types");
    assert_status(&response, 200);
    assert!(
        contains_named(&json_body(&response)["typeList"], "Aircraft"),
        "the registered type is present regardless of whether sources are contacted: {}",
        json_body(&response)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A registration whose remote holder has zero matching entities still lists the
/// type: registration-derived results are possibly-available (clauses 5.7.5).
/// Confirms REQ-CONS-81.
#[tokio::test]
async fn distop_available_types_possibly_available_semantics() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    let registration_id = topology.id("registration:cons81");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, peer.peer_url())
                    .mode("inclusive")
                    .entity_type("Aircraft")
                    .operations(&["retrieveEntity", "queryEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );

    let response = front.client().list_types("", Some(COMPOUND_CONTEXT.as_str())).await.expect("types");
    assert_status(&response, 200);
    assert!(
        contains_named(&json_body(&response)["typeList"], "Aircraft"),
        "a type promised only by registration is possibly-available and listed: {}",
        json_body(&response)
    );

    topology.finish().await.expect("topology cleanup");
}

/// A registration with an entity type but no attribute names permits empty
/// `attributeDetails`/`attributeTypes` arrays in the type details (clauses 5.7.7).
/// Confirms REQ-CONS-82.
#[tokio::test]
async fn distop_available_types_empty_arrays_permitted_when_registration_lacks_detail() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    let registration_id = topology.id("registration:cons82");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, peer.peer_url())
                    .mode("inclusive")
                    .entity_type("Aircraft")
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );

    let response = get_detail(&topology, "/ngsi-ld/v1/types/Aircraft").await;
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(
        body.get("attributeDetails").is_none_or(|details| details.is_array()),
        "empty attributeDetails is permitted, not an error: {body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// Two sources reporting overlapping counts yield a non-negative aggregate; an
/// estimate that may double-count is permitted (clauses 5.7.7). Confirms
/// REQ-CONS-83.
#[tokio::test]
async fn distop_available_counts_estimate_permitted() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    for (source, count) in [("/s1", 3), ("/s2", 4)] {
        let registration_id = topology.id(&format!("registration:cons83-{}", source.trim_start_matches('/')));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, &mock.url(source))
                        .mode("inclusive")
                        .entity_type("Aircraft")
                        .operations(&["retrieveEntity", "queryEntity"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
        mock.set_stub_with_body(
            Method::GET,
            &format!("{source}/ngsi-ld/v1/types/Aircraft"),
            200,
            &json!({ "id": "Aircraft", "type": "EntityTypeInfo", "typeName": "Aircraft", "entityCount": count }),
        )
        .await;
    }

    let response = get_detail(&topology, "/ngsi-ld/v1/types/Aircraft").await;
    assert_status(&response, 200);
    assert!(
        json_body(&response)["entityCount"].as_i64().is_none_or(|count| count >= 0),
        "the aggregated count is a non-negative estimate: {}",
        json_body(&response)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
