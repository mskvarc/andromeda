//! Federated write and redirection acceptance cases.

use crate::fixtures::{FedRole, assert_status, body, broker, entity, registration, topology};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial(fed)]
async fn fed_redirected_entity_crud_lifecycle_reaches_domain() {
    let topology = topology().await;
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let id = topology.id("Building:redirect-crud");
    let csr = registration(
        &topology.id("registration:redirect-crud"),
        madrid.peer_url(),
        "Building",
        None,
        "redirect",
        Some(&[
            "redirectionOps",
            "createEntity",
            "retrieveEntity",
            "updateEntity",
            "updateAttrs",
            "deleteEntity",
        ]),
    );
    assert_status(&hub.client().create_registration(&csr).await.expect("redirect registration"), 201);
    assert_status(
        &hub.client()
            .post_entity(&entity(&id, "Building", 21))
            .with_ld_json()
            .send()
            .await
            .expect("redirect create"),
        201,
    );
    assert_status(
        &madrid
            .client()
            .get_entity(&id, "", "application/ld+json", None)
            .await
            .expect("direct create proof"),
        200,
    );
    assert_status(
        &hub.client()
            .merge_entity(
                &id,
                &json!({"speed": {"type": "Property", "value": 22}, "@context": COMPOUND_CONTEXT.as_str()}),
                None,
            )
            .await
            .expect("redirect update"),
        204,
    );
    assert_eq!(
        body(
            &madrid
                .client()
                .get_entity(&id, "", "application/ld+json", None)
                .await
                .expect("direct update proof")
        )["speed"]["value"],
        22
    );
    assert_status(&hub.client().delete_entity(&id).await.expect("redirect delete"), 204);
    assert_status(
        &madrid
            .client()
            .get_entity(&id, "", "application/ld+json", None)
            .await
            .expect("direct delete proof"),
        404,
    );
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
#[serial(fed)]
async fn fed_batch_provision_routes_entities_to_three_domains() {
    let topology = topology().await;
    let hub = broker(&topology, FedRole::Hub);
    let domains = [(FedRole::Madrid, "MAD"), (FedRole::Berlin, "BER"), (FedRole::Nordic, "NOR")];
    for (role, prefix) in domains {
        let domain = broker(&topology, role);
        let pattern = format!(".*:{}-.*", prefix);
        let csr = json!({"id": topology.id(&format!("registration:batch:{prefix}")), "type": "ContextSourceRegistration", "endpoint": domain.peer_url(), "information": [{"entities": [{"type": "Building", "idPattern": pattern}]}], "mode": "redirect", "operations": ["redirectionOps", "createBatch", "updateBatch", "deleteBatch"], "@context": COMPOUND_CONTEXT.as_str()});
        assert_status(&hub.client().create_registration(&csr).await.expect("batch registration"), 201);
    }
    let entities: Vec<_> = ["MAD", "BER", "NOR"]
        .into_iter()
        .flat_map(|prefix| (0..100).map(move |value| entity(&format!("urn:ngsi-ld:test:{prefix}-{value}"), "Building", value)))
        .collect();
    let created = hub.client().batch_create(&json!(entities)).await.expect("batch create");
    assert!(created.status.as_u16() < 500, "batch create must not be a server failure: {}", created.status);
    for (role, prefix) in domains {
        let domain = broker(&topology, role);
        let query = format!("type=Building&idPattern=.*%3A{}-.*", prefix);
        let response = domain
            .client()
            .query_entities(&query, "application/ld+json", None)
            .await
            .expect("domain batch verification");
        assert_status(&response, 200);
        let response_body = body(&response);
        assert!(
            response_body.as_array().is_some_and(|entities| !entities.is_empty()),
            "{prefix} domain received no batch entities"
        );
    }
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
#[serial(fed)]
async fn fed_batch_write_reports_partial_domain_failure() {
    let topology = topology().await;
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    assert_status(
        &hub.client()
            .create_registration(&registration(
                &topology.id("registration:batch-success"),
                madrid.peer_url(),
                "Building",
                None,
                "redirect",
                Some(&["redirectionOps", "createBatch"]),
            ))
            .await
            .expect("success registration"),
        201,
    );
    let failing = json!({"id": topology.id("registration:batch-failure"), "type": "ContextSourceRegistration", "endpoint": "http://127.0.0.1:1", "information": [{"entities": [{"type": "Building", "idPattern": ".*:FAIL-.*"}]}], "mode": "redirect", "operations": ["redirectionOps", "createBatch"], "@context": COMPOUND_CONTEXT.as_str()});
    assert_status(&hub.client().create_registration(&failing).await.expect("failure registration"), 201);
    let response = hub
        .client()
        .batch_create(&json!([
            entity("urn:ngsi-ld:test:MAD-success", "Building", 1),
            entity("urn:ngsi-ld:test:FAIL-error", "Building", 2)
        ]))
        .await
        .expect("partial batch");
    assert_eq!(response.status.as_u16(), 207);
    assert!(body(&response)["errors"].as_array().is_some_and(|errors| {
        errors
            .iter()
            .any(|entry| entry["registrationId"].as_str().is_some_and(|id| id.contains("batch-failure")))
    }));
    assert_status(
        &madrid
            .client()
            .get_entity("urn:ngsi-ld:test:MAD-success", "", "application/ld+json", None)
            .await
            .expect("successful domain write"),
        200,
    );
    topology.finish().await.expect("topology cleanup");
}
