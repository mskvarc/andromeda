//! Federated write and redirection acceptance cases.

use crate::fixtures::{FedRole, assert_status, body, broker, entity, registration, topology};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use pretty_assertions::assert_eq;
use serde_json::json;
use urlencoding::encode;

#[tokio::test]
async fn fed_redirected_entity_crud_lifecycle_reaches_domain() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let id = topology.id("Building:redirect-crud");
    let mut csr = registration(
        &topology.id("registration:redirect-crud"),
        madrid.peer_url(),
        &btype,
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
    // A prefix idPattern confines this redirect registration's ownership to this
    // test's own entities, so it cannot intercept a concurrent test's writes or
    // its operations on absent entities of an unknown type.
    csr["information"][0]["entities"][0]["idPattern"] = json!(format!("^{}", topology.id_prefix()));
    assert_status(&hub.client().create_registration(&csr).await.expect("redirect registration"), 201);
    assert_status(
        &hub.client()
            .post_entity(&entity(&id, &btype, 21))
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
async fn fed_batch_provision_routes_entities_to_three_domains() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let domains = [(FedRole::Madrid, "MAD"), (FedRole::Berlin, "BER"), (FedRole::Nordic, "NOR")];
    // The routing idPattern is anchored to this test's own prefix so a concurrent
    // batch test's MAD/BER/NOR entities are neither routed here nor counted below.
    for (role, prefix) in domains {
        let domain = broker(&topology, role);
        let pattern = format!("^{}:{prefix}-.*", topology.id_prefix());
        let csr = json!({"id": topology.id(&format!("registration:batch:{prefix}")), "type": "ContextSourceRegistration", "endpoint": domain.peer_url(), "information": [{"entities": [{"type": btype, "idPattern": pattern}]}], "mode": "redirect", "operations": ["redirectionOps", "createBatch", "updateBatch", "deleteBatch"], "@context": COMPOUND_CONTEXT.as_str()});
        assert_status(&hub.client().create_registration(&csr).await.expect("batch registration"), 201);
    }
    let mut entities = Vec::new();
    for prefix in ["MAD", "BER", "NOR"] {
        for value in 0..100 {
            entities.push(entity(&topology.id(&format!("{prefix}-{value}")), &btype, value));
        }
    }
    let created = hub.client().batch_create(&json!(entities)).await.expect("batch create");
    assert!(created.status.as_u16() < 500, "batch create must not be a server failure: {}", created.status);
    for (role, prefix) in domains {
        let domain = broker(&topology, role);
        let query = format!(
            "type={}&idPattern={}",
            encode(&btype),
            encode(&format!("^{}:{prefix}-.*", topology.id_prefix()))
        );
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
async fn fed_batch_write_reports_partial_domain_failure() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let mad_success = topology.id("MAD-success");
    let fail_error = topology.id("FAIL-error");
    // Both registrations' idPatterns are anchored to this test's prefix so they
    // route and fail only this test's own MAD/FAIL entities.
    let mut success = registration(
        &topology.id("registration:batch-success"),
        madrid.peer_url(),
        &btype,
        None,
        "redirect",
        Some(&["redirectionOps", "createBatch"]),
    );
    success["information"][0]["entities"][0]["idPattern"] = json!(format!("^{}:MAD-.*", topology.id_prefix()));
    assert_status(&hub.client().create_registration(&success).await.expect("success registration"), 201);
    let failing = json!({"id": topology.id("registration:batch-failure"), "type": "ContextSourceRegistration", "endpoint": "http://127.0.0.1:1", "information": [{"entities": [{"type": btype, "idPattern": format!("^{}:FAIL-.*", topology.id_prefix())}]}], "mode": "redirect", "operations": ["redirectionOps", "createBatch"], "@context": COMPOUND_CONTEXT.as_str()});
    assert_status(&hub.client().create_registration(&failing).await.expect("failure registration"), 201);
    let response = hub
        .client()
        .batch_create(&json!([entity(&mad_success, &btype, 1), entity(&fail_error, &btype, 2)]))
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
            .get_entity(&mad_success, "", "application/ld+json", None)
            .await
            .expect("successful domain write"),
        200,
    );
    topology.finish().await.expect("topology cleanup");
}
