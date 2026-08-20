//! Registration update lifecycle (clause 5.9.3): a valid fragment merges and
//! returns 204, the `id` is immutable, a malformed target or fragment is
//! rejected, an unknown target is a 404, and shorthand names expand under the
//! request context. All `T-LOCAL`, all `IMPL`.

use crate::{
    oracle::{assert_problem_details, assert_status, json_body},
    reg::PEER_ENDPOINT,
    registration::csr_basic,
    topology::DistributedTopology,
};
use andromeda_test_fixtures::{
    context::COMPOUND_CONTEXT,
    problem_detail::{BAD_REQUEST_DATA, RESOURCE_NOT_FOUND},
};
use reqwest::Method;
use serde_json::{Value, json};
use urlencoding::encode;

/// Create the baseline registration under the given id.
async fn create_baseline(topology: &DistributedTopology, id: &str) {
    let created = topology
        .front()
        .client()
        .create_registration(&csr_basic(id, PEER_ENDPOINT))
        .await
        .expect("create registration");
    assert_status(&created, 201);
}

/// A valid update fragment returns 204 and merges into the stored registration
/// (clauses 5.9.3, 5.5.8). Confirms REQ-REG-66, REQ-REG-77, REQ-REG-78.
#[tokio::test]
async fn update_valid_fragment_returns_204() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("registration:update-valid");
    create_baseline(&topology, &id).await;

    let fragment = json!({ "description": "updated telemetry source" });
    let updated = topology
        .front()
        .client()
        .update_registration(&id, &fragment, None)
        .await
        .expect("patch registration");
    assert_status(&updated, 204);
    assert!(updated.body.is_empty(), "a successful update returns an empty body");

    let retrieved = topology.front().client().retrieve_registration(&id, None).await.expect("retrieve registration");
    assert_status(&retrieved, 200);
    assert_eq!(
        json_body(&retrieved)["description"].as_str(),
        Some("updated telemetry source"),
        "the fragment is merged into the registration"
    );

    topology.finish().await.expect("topology cleanup");
}

/// An update fragment cannot change the registration `id`: the original id keeps
/// resolving and the proposed id is never created (clause 5.9.3). Confirms
/// REQ-REG-7.
#[tokio::test]
async fn update_cannot_change_id() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id_a = topology.id("registration:update-id-a");
    let id_b = topology.id("registration:update-id-b");
    create_baseline(&topology, &id_a).await;

    let fragment = json!({ "id": id_b });
    let _patched = topology
        .front()
        .client()
        .update_registration(&id_a, &fragment, None)
        .await
        .expect("patch with a new id");

    let under_a = topology
        .front()
        .client()
        .retrieve_registration(&id_a, None)
        .await
        .expect("retrieve original id");
    assert_status(&under_a, 200);
    assert_eq!(json_body(&under_a)["id"].as_str(), Some(id_a.as_str()), "the original id is unmodified");
    let under_b = topology
        .front()
        .client()
        .retrieve_registration(&id_b, None)
        .await
        .expect("retrieve proposed id");
    assert_status(&under_b, 404);

    topology.finish().await.expect("topology cleanup");
}

/// An update against a syntactically invalid target id is rejected (clause
/// 5.9.3). Confirms REQ-REG-67.
#[tokio::test]
async fn update_invalid_target_id_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let fragment = json!({ "description": "irrelevant" });
    let rejected = topology
        .front()
        .client()
        .update_registration("not-a-uri", &fragment, None)
        .await
        .expect("patch invalid target");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);

    topology.finish().await.expect("topology cleanup");
}

/// An update against a well-formed but unknown registration id is a 404 (clause
/// 5.9.3). Confirms REQ-REG-69.
#[tokio::test]
async fn update_unknown_registration_returns_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let unknown = topology.id("registration:update-unknown");
    let fragment = json!({ "description": "irrelevant" });
    let not_found = topology
        .front()
        .client()
        .update_registration(&unknown, &fragment, None)
        .await
        .expect("patch unknown registration");
    assert_status(&not_found, 404);
    assert_problem_details(&not_found, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// An update fragment with a `contextSourceInfo` `accept` that violates 4.3.6.6
/// is rejected (clause 5.9.3). Confirms REQ-REG-68.
#[tokio::test]
async fn update_bad_contextsourceinfo_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("registration:update-bad-csi");
    create_baseline(&topology, &id).await;

    let fragment = json!({ "contextSourceInfo": [ { "key": "accept", "value": "text/plain" } ] });
    let rejected = topology
        .front()
        .client()
        .update_registration(&id, &fragment, None)
        .await
        .expect("patch bad contextSourceInfo");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);

    topology.finish().await.expect("topology cleanup");
}

/// An `application/ld+json` update whose `@context` is not a valid JSON-LD context
/// is rejected (clauses 5.9.3, 5.5.4). Confirms REQ-REG-70.
#[tokio::test]
async fn update_invalid_jsonld_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("registration:update-bad-jsonld");
    create_baseline(&topology, &id).await;

    let path = format!("/ngsi-ld/v1/csourceRegistrations/{}", encode(&id));
    let rejected = topology
        .front()
        .client()
        .raw(Method::PATCH, &path)
        .header("content-type", "application/ld+json")
        .json(&json!({ "description": "irrelevant", "@context": 5 }))
        .send()
        .await
        .expect("raw ld+json patch");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);

    topology.finish().await.expect("topology cleanup");
}

/// An update fragment that violates a 5.2.9 constraint is rejected (clause
/// 5.9.3). Confirms REQ-REG-71.
#[tokio::test]
async fn update_fragment_violates_529_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("registration:update-violates-529");
    create_baseline(&topology, &id).await;

    let fragment = json!({ "mode": "foo" });
    let rejected = topology
        .front()
        .client()
        .update_registration(&id, &fragment, None)
        .await
        .expect("patch invalid mode");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);

    topology.finish().await.expect("topology cleanup");
}

/// Shorthand names in an update fragment are expanded through the request context
/// and stored as IRIs (clauses 5.9.3, 5.5.7). Confirms REQ-REG-72.
#[tokio::test]
async fn update_shorthand_names_expanded() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("registration:update-shorthand");
    create_baseline(&topology, &id).await;

    let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle" } ], "propertyNames": ["speed"] } ] });
    let updated = topology
        .front()
        .client()
        .update_registration(&id, &fragment, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("patch shorthand names");
    assert_status(&updated, 204);

    let retrieved = topology
        .front()
        .client()
        .retrieve_registration(&id, None)
        .await
        .expect("retrieve without context");
    assert_status(&retrieved, 200);
    let stored = json_body(&retrieved);
    let names: Vec<&str> = stored["information"][0]["propertyNames"]
        .as_array()
        .unwrap_or_else(|| unreachable!("propertyNames round-trips as an array"))
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        names.iter().any(|name| name.contains(':') && *name != "speed"),
        "the shorthand name expanded to an IRI when read back without the mapping context, got {names:?}"
    );

    topology.finish().await.expect("topology cleanup");
}
