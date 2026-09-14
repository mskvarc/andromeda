//! Retrieve-CSR (clause 5.10.1): id validation, existence, the single-object
//! JSON-LD result, and short-hand term expansion (clause 5.5.7). All `T-LOCAL`.

use crate::{
    disc::PEER_ENDPOINT,
    oracle::{assert_problem_details, assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::{
    context::COMPOUND_CONTEXT,
    problem_detail::{BAD_REQUEST_DATA, RESOURCE_NOT_FOUND},
};
use pretty_assertions::assert_eq;

/// A malformed registration id is rejected with `400 BadRequestData` before any
/// lookup (clause 5.10.1). Confirms REQ-DISC-13.
#[tokio::test]
async fn retrieve_csr_invalid_uri_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration("not a uri", None)
        .await
        .expect("retrieve registration");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A syntactically valid but unregistered id returns `404 ResourceNotFound`
/// (clause 5.10.1). Confirms REQ-DISC-14.
#[tokio::test]
async fn retrieve_csr_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let unknown = topology.id("registration:absent");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(&unknown, None)
        .await
        .expect("retrieve registration");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// Retrieved without a context the custom terms render as fully-qualified IRIs,
/// and with the creation context they compact back to short-hand (clause
/// 5.5.7). Confirms REQ-DISC-15.
#[tokio::test]
async fn retrieve_csr_expands_shorthand_terms_in_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:expansion");
    let body = CsrBuilder::new(&id, PEER_ENDPOINT).entity_type("Vehicle").property_names(&["speed"]).build();
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration");
    assert_status(&created, 201);

    let expanded = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .without_default_context()
        .retrieve_registration(&id, None)
        .await
        .expect("retrieve without context");
    assert_status(&expanded, 200);
    let expanded = json_body(&expanded);
    let expanded_type = expanded["information"][0]["entities"][0]["type"]
        .as_str()
        .unwrap_or_else(|| panic!("entity type present"));
    assert!(
        expanded_type.contains("://") && expanded_type != "Vehicle",
        "without a context the entity type is a full IRI, got {expanded_type}"
    );

    let compacted = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(&id, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("retrieve with context");
    assert_status(&compacted, 200);
    let compacted = json_body(&compacted);
    assert_eq!(
        compacted["information"][0]["entities"][0]["type"].as_str(),
        Some("Vehicle"),
        "with the compound context the entity type compacts to short-hand"
    );
    topology.finish().await.expect("topology cleanup");
}

/// An existing registration is returned as a single JSON-LD object carrying its
/// id and `ContextSourceRegistration` type (clauses 5.10.1, 5.2.9). Confirms
/// REQ-DISC-16.
#[tokio::test]
async fn retrieve_csr_existing_returns_200_jsonld_object() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:existing");
    let body = CsrBuilder::new(&id, PEER_ENDPOINT).build();
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration");
    assert_status(&created, 201);

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(&id, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("retrieve registration");
    assert_status(&retrieved, 200);
    let retrieved = json_body(&retrieved);
    assert!(retrieved.is_object(), "a single registration is a JSON-LD object, got {retrieved}");
    assert_eq!(retrieved["id"].as_str(), Some(id.as_str()), "the retrieved object carries the requested id");
    assert_eq!(
        retrieved["type"].as_str(),
        Some("ContextSourceRegistration"),
        "the retrieved object is a ContextSourceRegistration"
    );
    topology.finish().await.expect("topology cleanup");
}
