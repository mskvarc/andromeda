//! Batch Create forwarding (clause 5.6.7.4).
//!
//! Each matching registration receives its own filtered copy of the batch: an
//! exclusive/redirect source is forwarded only the entities and attributes it
//! owns, entities no source matches are dropped, and a source owning nothing in
//! the batch is skipped. A source advertising the batch operation is forwarded
//! the batch and its per-entity results are merged with a `registrationId`;
//! otherwise the broker falls back to per-entity creates, and a redirect
//! source advertising neither yields a per-entity `409 Conflict`. The local
//! phase populates `success`/`errors` independently of forwarding.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, attr_present, entity_with_id, forwarded_entities, request_path},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// Per-CSR filtering forwards each redirect source only its owned attribute of
/// the matching entity, dropping the unmatched entity entirely.
#[tokio::test]
async fn batch_create_per_csr_filters_input_copy() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:prov-batch-create-filter-v1");
    let d1 = topology.id("Device:prov-batch-create-filter-d1");
    let dtype = topology.id("Device");
    let reg_speed = topology.id("registration:prov-batch-create-filter-speed");
    let reg_location = topology.id("registration:prov-batch-create-filter-location");

    let speed_csr = CsrBuilder::new(&reg_speed, &mock.url("/s1"))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["speed"])
        .operations(&["createBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&speed_csr).await.expect("create speed registration"), 201);
    let location_csr = CsrBuilder::new(&reg_location, &mock.url("/s2"))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["location"])
        .operations(&["createBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&location_csr).await.expect("create location registration"), 201);
    mock.set_stub(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create", 201).await;
    mock.set_stub(Method::POST, "/s2/ngsi-ld/v1/entityOperations/create", 201).await;

    let batch = json!([
        entity(
            &v1,
            &vtype,
            &[
                ("brandName", property(json!("Mercedes"))),
                ("speed", property(json!(42))),
                ("location", property(json!("here")))
            ]
        ),
        entity(&d1, &dtype, &[("brandName", property(json!("sensor")))])
    ]);
    client.batch_create(&batch).await.expect("batch create");

    let first = mock.next_request(FORWARD_WAIT).await;
    let second = mock.next_request(FORWARD_WAIT).await;
    let (s1_request, s2_request) = if request_path(&first).contains("/s1") {
        (first, second)
    } else {
        (second, first)
    };
    let s1_body = s1_request.json::<Value>().expect("s1 forwarded body is JSON");
    let s2_body = s2_request.json::<Value>().expect("s2 forwarded body is JSON");

    let s1_entities = forwarded_entities(&s1_body);
    let s1_vehicle = entity_with_id(&s1_entities, &v1).expect("s1 receives the vehicle");
    assert!(attr_present(s1_vehicle, "speed"), "the speed source receives speed: {s1_body}");
    assert!(!attr_present(s1_vehicle, "location"), "the speed source does not receive location: {s1_body}");
    assert!(
        !attr_present(s1_vehicle, "brandName"),
        "the redirect copy strips the unowned brandName: {s1_body}"
    );
    assert!(entity_with_id(&s1_entities, &d1).is_none(), "the unmatched Device is dropped: {s1_body}");

    let s2_entities = forwarded_entities(&s2_body);
    let s2_vehicle = entity_with_id(&s2_entities, &v1).expect("s2 receives the vehicle");
    assert!(attr_present(s2_vehicle, "location"), "the location source receives location: {s2_body}");
    assert!(!attr_present(s2_vehicle, "speed"), "the location source does not receive speed: {s2_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect-owned attribute is stripped from the copy forwarded to another
/// matching source.
#[tokio::test]
async fn batch_create_strips_redirect_owned_attrs_for_other_csr() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:prov-batch-create-redir-v1");
    let reg_speed = topology.id("registration:prov-batch-create-redir-speed");
    let reg_brand = topology.id("registration:prov-batch-create-redir-brand");

    let speed_csr = CsrBuilder::new(&reg_speed, &mock.url("/s1"))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["speed"])
        .operations(&["createBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&speed_csr).await.expect("create speed registration"), 201);
    let brand_csr = CsrBuilder::new(&reg_brand, &mock.url("/s2"))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["brandName"])
        .operations(&["createBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&brand_csr).await.expect("create brand registration"), 201);
    mock.set_stub(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create", 201).await;
    mock.set_stub(Method::POST, "/s2/ngsi-ld/v1/entityOperations/create", 201).await;

    let batch = json!([entity(
        &v1,
        &vtype,
        &[("brandName", property(json!("Mercedes"))), ("speed", property(json!(42)))]
    )]);
    client.batch_create(&batch).await.expect("batch create");

    let first = mock.next_request(FORWARD_WAIT).await;
    let second = mock.next_request(FORWARD_WAIT).await;
    let s2_request = if request_path(&first).contains("/s2") { first } else { second };
    let s2_body = s2_request.json::<Value>().expect("s2 forwarded body is JSON");
    let s2_entities = forwarded_entities(&s2_body);
    let s2_vehicle = entity_with_id(&s2_entities, &v1).expect("s2 receives the vehicle");
    assert!(
        attr_present(s2_vehicle, "brandName"),
        "the brand source receives its owned attribute: {s2_body}"
    );
    assert!(
        !attr_present(s2_vehicle, "speed"),
        "the redirect-owned speed is stripped from the other copy: {s2_body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source that owns nothing present in the batch is skipped, while the other
/// source is still forwarded.
#[tokio::test]
async fn batch_create_empty_in_skips_csr() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:prov-batch-create-skip-v1");
    let reg_colour = topology.id("registration:prov-batch-create-skip-colour");
    let reg_speed = topology.id("registration:prov-batch-create-skip-speed");

    let colour_csr = CsrBuilder::new(&reg_colour, &mock.url("/s1"))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["colour"])
        .operations(&["createBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&colour_csr).await.expect("create colour registration"), 201);
    let speed_csr = CsrBuilder::new(&reg_speed, &mock.url("/s2"))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["speed"])
        .operations(&["createBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&speed_csr).await.expect("create speed registration"), 201);
    mock.set_stub(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create", 201).await;
    mock.set_stub(Method::POST, "/s2/ngsi-ld/v1/entityOperations/create", 201).await;

    let batch = json!([entity(&v1, &vtype, &[("speed", property(json!(42)))])]);
    client.batch_create(&batch).await.expect("batch create");

    assert_eq!(
        mock.get_count(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create"),
        0,
        "the source owning only the absent colour is skipped"
    );
    assert!(
        mock.get_count(Method::POST, "/s2/ngsi-ld/v1/entityOperations/create") >= 1,
        "the source owning speed is still forwarded"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source advertising the batch operation is forwarded the batch and its
/// per-entity results merge into the response keyed by `registrationId`.
#[tokio::test]
async fn batch_create_supported_forwards_and_merges_results() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:prov-batch-create-merge-v1");
    let v2 = topology.id("Vehicle:prov-batch-create-merge-v2");
    let reg_id = topology.id("registration:prov-batch-create-merge");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .operations(&["createBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let remote_result = json!({
        "success": [v2],
        "errors": [ { "entityId": v1, "error": { "status": 409, "type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists", "title": "exists" } } ]
    });
    mock.set_stub_with_body(Method::POST, "/ngsi-ld/v1/entityOperations/create", 207, &remote_result)
        .await;

    let batch = json!([
        entity(&v1, &vtype, &[("speed", property(json!(1)))]),
        entity(&v2, &vtype, &[("speed", property(json!(2)))])
    ]);
    let response = client.batch_create(&batch).await.expect("batch create");
    assert_status(&response, 207);
    let body = json_body(&response);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert!(
        errors.iter().any(|entry| entry["registrationId"] == json!(reg_id)),
        "the merged remote error is keyed by registrationId: {body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Without the batch operation the broker falls back to one forwarded create per
/// entity.
#[tokio::test]
async fn batch_create_fallback_per_entity_create() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:prov-batch-create-fallback-v1");
    let v2 = topology.id("Vehicle:prov-batch-create-fallback-v2");
    let reg_id = topology.id("registration:prov-batch-create-fallback");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    let batch = json!([
        entity(&v1, &vtype, &[("speed", property(json!(1)))]),
        entity(&v2, &vtype, &[("speed", property(json!(2)))])
    ]);
    client.batch_create(&batch).await.expect("batch create");

    assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entities"),
        2,
        "the batch fell back to one create per entity"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect source advertising neither the batch nor the per-entity create
/// yields a per-entity `409 Conflict` keyed by `registrationId`, with no
/// forwarding.
#[tokio::test]
async fn batch_create_redirect_neither_op_conflict_per_entity() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:prov-batch-create-neither-v1");
    let v2 = topology.id("Vehicle:prov-batch-create-neither-v2");
    let reg_id = topology.id("registration:prov-batch-create-neither");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let batch = json!([
        entity(&v1, &vtype, &[("speed", property(json!(1)))]),
        entity(&v2, &vtype, &[("speed", property(json!(2)))])
    ]);
    let response = client.batch_create(&batch).await.expect("batch create");
    assert_status(&response, 207);
    let body = json_body(&response);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    let conflicts = errors
        .iter()
        .filter(|entry| entry["error"]["status"] == json!(409) && entry["registrationId"] == json!(reg_id))
        .count();
    assert_eq!(conflicts, 2, "each entity surfaces a keyed conflict: {body}");
    assert_eq!(mock.get_total_count(), 0, "no forwarding on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local phase populates `success` for the new entity and `errors` for the
/// pre-existing one, independently of the auxiliary registration.
#[tokio::test]
async fn batch_create_local_phase_populates_success_and_errors() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:prov-batch-create-local-v1");
    let v2 = topology.id("Vehicle:prov-batch-create-local-v2");
    let reg_id = topology.id("registration:prov-batch-create-local");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(&v2, &vtype, &[("speed", property(json!(9)))]))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("auxiliary")
        .entity_type(&vtype)
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let batch = json!([
        entity(&v1, &vtype, &[("speed", property(json!(1)))]),
        entity(&v2, &vtype, &[("speed", property(json!(2)))])
    ]);
    let response = client.batch_create(&batch).await.expect("batch create");
    assert_status(&response, 207);
    let body = json_body(&response);
    assert!(
        body["success"]
            .as_array()
            .is_some_and(|success| success.iter().any(|value| value == &json!(v1) || value["entityId"] == json!(v1))),
        "the new entity is a local success: {body}"
    );
    assert!(
        body["errors"].as_array().is_some_and(|errors| errors
            .iter()
            .any(|entry| entry["entityId"] == json!(v2) && entry["error"]["status"] == json!(409))),
        "the pre-existing entity is a local conflict: {body}"
    );
    assert_eq!(mock.get_total_count(), 0, "the auxiliary registration does not forward provision operations");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
