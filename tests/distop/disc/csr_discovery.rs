//! The Query-CSR test surface: registering a context source and reading which
//! registrations the registry returns.
//!
//! `GET /csourceRegistrations` answers with a JSON-LD array of the Context
//! Source Registrations that match the discovery query (clause 5.10.2). Because
//! the distributed suite shares one database across concurrently running tests,
//! DISC assertions never rely on the array's exact length: they check that the
//! registration a test created is present or absent by its id, which is robust
//! against unrelated registrations that happen to coexist under other prefixes.

use crate::{
    disc::PEER_ENDPOINT,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::{Value, json};

/// Create a Context Source Registration on the front broker and assert the
/// `201 Created` acknowledgement.
pub async fn register(topology: &DistributedTopology, body: &Value) {
    let created = topology.front().client().create_registration(body).await.expect("create registration");
    assert_status(&created, 201);
}

/// Register a CSR of the given type carrying a `location` Point, so geo-queries
/// have a GeoProperty to match against.
pub async fn register_located(topology: &DistributedTopology, id: &str, entity_type: &str, coordinates: [f64; 2]) {
    let mut body = CsrBuilder::new(id, PEER_ENDPOINT).entity_type(entity_type).build();
    body["location"] = json!({ "type": "Point", "coordinates": coordinates });
    register(topology, &body).await;
}

/// Register a CSR of the given type carrying a temporal interval field
/// (`observationInterval` or `managementInterval`).
pub async fn register_interval(topology: &DistributedTopology, id: &str, entity_type: &str, field: &str, start: &str, end: &str) {
    let mut body = CsrBuilder::new(id, PEER_ENDPOINT).entity_type(entity_type).build();
    body[field] = json!({ "startAt": start, "endAt": end });
    register(topology, &body).await;
}

/// Run a Query-CSR with the compound test context applied, assert `200 OK`, and
/// return the decoded JSON-LD array body.
pub async fn query(topology: &DistributedTopology, query: &str) -> Value {
    let response = topology
        .front()
        .client()
        .query_registrations(query, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("query registrations");
    assert_status(&response, 200);
    json_body(&response)
}

/// The `id` of every registration in a Query-CSR result array.
pub fn matched_ids(body: &Value) -> Vec<String> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|registration| registration["id"].as_str())
        .map(str::to_owned)
        .collect()
}

/// Whether the Query-CSR result contains the registration with this id.
pub fn contains(body: &Value, id: &str) -> bool {
    matched_ids(body).iter().any(|matched| matched == id)
}

/// Borrow the registration with this id from a Query-CSR result array.
pub fn find<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.as_array()
        .into_iter()
        .flatten()
        .find(|registration| registration["id"].as_str() == Some(id))
}
