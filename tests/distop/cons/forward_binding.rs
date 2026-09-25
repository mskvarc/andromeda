//! Operation-aware forward binding: the exact shape of the request a broker
//! forwards when a registration advertises only some of the read operations
//! (ETSI GS CIM 009 v1.9.1, clauses 4.3.6.1, 6.23, Table 4.20-2).
//!
//! Clause 4.3.6.1: "registered Context Sources may indicate that they are only
//! willing to respond to a limited subset of API operations ... Context Brokers
//! shall respect this", and "all constraints specified in the registration shall
//! be respected". A registration advertising only `queryEntity` (not
//! `retrieveEntity`), or only `queryBatch` (neither), must still be contacted for
//! a read the client made, through an operation it does allow. These tests
//! capture the forwarded request with the mock and assert its resource path and
//! body/query byte-for-byte, rather than merely counting that the source was
//! reached: the redesign's behaviour is the *shape* of the downgraded request.
//!
//! The distributed client carries the compound test `@context` as its default,
//! so the covered type `Vehicle` is expanded against it at the broker and
//! compacted back to `Vehicle` in the forwarded request (clause 4.3.6.1).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The forwarded collection path for entity query and create (clause 6.3.7).
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// The forwarded path of a Batch Entity Query (clause 6.23.3.1).
const FORWARDED_BATCH_QUERY: &str = "/ngsi-ld/v1/entityOperations/query";

/// A minimal NGSI-LD `Query` (clause 5.2.23) naming one entity type scoped to a
/// per-test id prefix, carrying the compound context inline for
/// `application/ld+json` submission. The `idPattern` keeps a bare-type query from
/// matching another concurrent test's registrations covering the same type.
fn type_query(entity_type: &str, id_pattern: &str) -> Value {
    json!({
        "type": "Query",
        "entities": [{ "type": entity_type, "idPattern": id_pattern }],
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// Clause 4.3.6.1 / 6.23.1: a retrieve-by-id sent to a registration advertising
/// only `queryEntity` is downgraded to a GET query naming the entity's id and its
/// compacted registered type, not a retrieve-by-id the source never registered.
// The forwarded-request compaction assertions require the literal shared type
// `Vehicle`, so this test cannot derive a per-test type; it is serialised with
// the other literal-type tests instead of run concurrently with them.
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn forward_retrieve_downgrades_to_query_naming_id_and_type() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let entity_id = topology.id("Vehicle:bind-retrieve-query");
    let registration = CsrBuilder::new(&topology.id("registration:bind-retrieve-query"), &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    // A retrieve-by-id whose only matching registration advertises queryEntity is
    // reached through a GET query, so no retrieve-by-id path is contacted.
    let _ = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded.uri.path(),
        FORWARDED_ENTITIES,
        "the retrieve is downgraded to a GET query, not a retrieve-by-id"
    );
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(
        forwarded_query.contains("id="),
        "the downgraded query names the entity by id: {forwarded_query}"
    );
    assert!(
        forwarded_query.contains("bind-retrieve-query"),
        "the downgraded query carries the retrieved id: {forwarded_query}"
    );
    assert!(
        forwarded_query.contains("type=Vehicle"),
        "the downgraded query names the compacted registered type: {forwarded_query}"
    );
    assert_eq!(
        mock.get_count(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}")),
        0,
        "no retrieve-by-id request is forwarded to a queryEntity-only source"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 4.3.6.1 / 6.23: a retrieve-by-id sent to a registration advertising
/// only `queryBatch` is downgraded to a POST `entityOperations/query` whose
/// clause-5.2.23 `Query` body names the entity by its compacted type and id.
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn forward_retrieve_downgrades_to_batch_query_body() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_BATCH_QUERY, 200, &json!([])).await;

    let entity_id = topology.id("Vehicle:bind-retrieve-batch");
    let registration = CsrBuilder::new(&topology.id("registration:bind-retrieve-batch"), &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["queryBatch"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let _ = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.uri.path(), FORWARDED_BATCH_QUERY, "the retrieve is downgraded to a batch query");
    let body = forwarded.json::<Value>().expect("the forwarded batch query carries a JSON body");
    assert_eq!(body["type"], "Query", "the batch body is a clause-5.2.23 Query: {body}");
    let selectors = body["entities"].as_array().expect("the Query names its entity selectors");
    assert_eq!(selectors.len(), 1, "one selector names the retrieved entity: {body}");
    assert_eq!(selectors[0]["type"], "Vehicle", "the selector names the compacted registered type: {body}");
    assert_eq!(selectors[0]["id"], json!(entity_id), "the selector names the retrieved id: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 4.3.6.1 / 6.23: a GET query sent to a registration advertising only
/// `queryBatch` is forwarded as a POST `entityOperations/query` whose `Query`
/// body carries one selector per queried id.
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn forward_get_query_downgrades_to_batch_query_body() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_BATCH_QUERY, 200, &json!([])).await;

    let entity_id = topology.id("Vehicle:bind-get-batch");
    let registration = CsrBuilder::new(&topology.id("registration:bind-get-batch"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Vehicle")
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .operations(&["queryBatch"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let query = format!("type=Vehicle&id={entity_id}");
    let response = front.client().query_entities(&query, "application/ld+json", None).await.expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.uri.path(), FORWARDED_BATCH_QUERY, "the GET query is forwarded as a batch query");
    let body = forwarded.json::<Value>().expect("the forwarded batch query carries a JSON body");
    assert_eq!(body["type"], "Query", "the batch body is a clause-5.2.23 Query: {body}");
    let selectors = body["entities"].as_array().expect("the Query names its entity selectors");
    assert_eq!(selectors.len(), 1, "the single queried id yields a single selector: {body}");
    assert_eq!(selectors[0]["id"], json!(entity_id), "the selector carries the queried id: {body}");
    assert_eq!(selectors[0]["type"], "Vehicle", "the selector carries the compacted covered type: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 4.3.6.1 / 5.7.2.4: a POST `entityOperations/query` sent to a
/// registration advertising only `queryEntity` is downgraded to a GET query, the
/// operation the registration actually allows, and never reaches the batch-query
/// endpoint.
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn forward_post_query_downgrades_to_get_query() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let registration = CsrBuilder::new(&topology.id("registration:bind-post-get"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Vehicle")
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let response = front
        .client()
        .batch_query(&type_query("Vehicle", &format!("^{}", topology.id_prefix())), "application/ld+json", None)
        .await
        .expect("batch query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded.uri.path(),
        FORWARDED_ENTITIES,
        "a queryEntity-only source is reached through a GET query, not a batch query"
    );
    assert!(
        forwarded.uri.query().unwrap_or_default().contains("type=Vehicle"),
        "the downgraded GET query names the covered type: {}",
        forwarded.uri.query().unwrap_or_default()
    );
    assert_eq!(
        mock.get_count(Method::POST, FORWARDED_BATCH_QUERY),
        0,
        "no batch query is forwarded to a queryEntity-only source"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 4.3.6.1 / 6.23: a POST `entityOperations/query` sent to a registration
/// advertising `queryBatch` is forwarded as a POST batch query with a valid
/// clause-6.23 `Query` body -- the operation the client used and the source
/// registered.
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn forward_post_query_prefers_batch_when_advertised() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_BATCH_QUERY, 200, &json!([])).await;

    let registration = CsrBuilder::new(&topology.id("registration:bind-post-batch"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Vehicle")
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .operations(&["queryBatch"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let response = front
        .client()
        .batch_query(&type_query("Vehicle", &format!("^{}", topology.id_prefix())), "application/ld+json", None)
        .await
        .expect("batch query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded.uri.path(),
        FORWARDED_BATCH_QUERY,
        "a queryBatch source keeps the client's batch-query binding"
    );
    let body = forwarded.json::<Value>().expect("the forwarded batch query carries a JSON body");
    assert_eq!(body["type"], "Query", "the forwarded body is a clause-6.23 Query: {body}");
    let selectors = body["entities"].as_array().expect("the Query names its entity selectors");
    assert!(
        selectors.iter().any(|selector| selector["type"] == json!("Vehicle")),
        "the forwarded Query names the covered type: {body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 4.3.6.3: an exclusive registration whose block names two entity ids is
/// narrowed to that block's footprint -- a type-only query forwarded as a batch
/// query names one selector per covered id, and no id the block does not cover.
#[tokio::test]
async fn forward_multi_id_exclusive_block_names_each_id() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_BATCH_QUERY, 200, &json!([])).await;

    let vtype = topology.id("Vehicle");
    let covered_a = topology.id("Vehicle:bind-multi-A");
    let covered_b = topology.id("Vehicle:bind-multi-B");
    // Clause 4.3.6.3 reads per-EntityInfo: one exclusive registration may name
    // several entities, each by its own id, in a single information block.
    let registration = json!({
        "id": topology.id("registration:bind-multi"),
        "type": "ContextSourceRegistration",
        "endpoint": mock.url(""),
        "mode": "exclusive",
        "operations": ["queryBatch"],
        "information": [{
            "entities": [
                { "type": vtype, "id": covered_a },
                { "type": vtype, "id": covered_b }
            ],
            "propertyNames": ["speed"]
        }],
        "@context": COMPOUND_CONTEXT.as_str()
    });
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let response = front
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.uri.path(), FORWARDED_BATCH_QUERY, "the exclusive block is forwarded as a batch query");
    let body = forwarded.json::<Value>().expect("the forwarded batch query carries a JSON body");
    let selectors = body["entities"].as_array().expect("the Query names its entity selectors");
    assert_eq!(selectors.len(), 2, "the block's two covered ids yield two selectors: {body}");
    let ids: Vec<&str> = selectors.iter().filter_map(|selector| selector["id"].as_str()).collect();
    assert!(ids.contains(&covered_a.as_str()), "the first covered id is named: {body}");
    assert!(ids.contains(&covered_b.as_str()), "the second covered id is named: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
