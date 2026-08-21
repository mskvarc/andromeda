//! Purge forwarding — REQ-PROV-130..133 (clause 5.6.21.4).
//!
//! `POST /entityOperations/purge` deletes matched entities and, unless the
//! request is `local=true`, forwards a purge to every matching registration that
//! supports it. The whole area is first-run UNKNOWN, so success statuses are
//! asserted tolerantly while the forwarding gate, the per-registration fan-out,
//! the unsupported-operation branch, and the local query-then-delete phase are
//! each asserted directly.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::assert_status,
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use axum::http::Method;
use serde_json::json;

/// The JSON-LD `Link` header carrying the compound test context for a purge
/// request, which has no body to hold an inline `@context`.
fn context_link() -> String {
    format!(
        r#"<{}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#,
        COMPOUND_CONTEXT.as_str()
    )
}

/// A `local=true` purge skips forwarding entirely while still deleting the
/// locally matched entity. Confirms REQ-PROV-130 (local-scope gate).
#[tokio::test]
async fn purge_local_scope_skips_forwarding() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-purge-local");
    let reg_id = topology.id("registration:prov-purge-local");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(&entity_id, "Vehicle", &[("speed", property(json!(42)))]))
        .send()
        .await
        .expect("seed local");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .operations(&["deleteEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let purged = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/purge?type=Vehicle&local=true")
        .header("content-type", "application/ld+json")
        .header("link", context_link().as_str())
        .body(Vec::new())
        .send()
        .await
        .expect("purge");
    assert!(
        matches!(purged.status.as_u16(), 200 | 204 | 207),
        "a local purge succeeds: {}",
        String::from_utf8_lossy(&purged.body)
    );
    assert_eq!(mock.get_total_count().await, 0, "a local-scope purge forwards nothing");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 404);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-local purge forwards a purge to every matching registration that
/// supports it. Confirms REQ-PROV-130 (support gate), 131.
#[tokio::test]
async fn purge_non_local_matching_csr_forwards_per_registration() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let reg_id_one = topology.id("registration:prov-purge-fanout-1");
    let reg_id_two = topology.id("registration:prov-purge-fanout-2");

    let registration_one = CsrBuilder::new(&reg_id_one, &mock.url("/s1"))
        .mode("redirect")
        .operations(&["deleteEntity", "queryBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration_one).await.expect("create registration one"), 201);
    let registration_two = CsrBuilder::new(&reg_id_two, &mock.url("/s2"))
        .mode("redirect")
        .operations(&["deleteEntity", "queryBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration_two).await.expect("create registration two"), 201);

    mock.set_stub(Method::POST, "/s1/ngsi-ld/v1/entityOperations/purge", 204).await;
    mock.set_stub(Method::POST, "/s2/ngsi-ld/v1/entityOperations/purge", 204).await;

    let purged = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/purge?type=Vehicle")
        .header("content-type", "application/ld+json")
        .header("link", context_link().as_str())
        .body(Vec::new())
        .send()
        .await
        .expect("purge");
    assert!(
        matches!(purged.status.as_u16(), 200 | 204 | 207),
        "a non-local purge succeeds: {}",
        String::from_utf8_lossy(&purged.body)
    );

    assert!(
        mock.get_count(Method::POST, "/s1/ngsi-ld/v1/entityOperations/purge").await >= 1,
        "the first registration is forwarded a purge"
    );
    assert!(
        mock.get_count(Method::POST, "/s2/ngsi-ld/v1/entityOperations/purge").await >= 1,
        "the second registration is forwarded a purge"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A matching registration that does not advertise the purge operation yields a
/// `409 Conflict` (bare) or a keyed `207 Multi-Status`, with no forwarding.
/// Confirms REQ-PROV-132.
#[tokio::test]
async fn purge_op_unsupported_returns_conflict_or_partial() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let reg_id = topology.id("registration:prov-purge-unsup");

    let registration = CsrBuilder::new(&reg_id, &mock.url("")).mode("redirect").operations(&["retrieveEntity"]).build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let purged = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/purge?type=Vehicle")
        .header("content-type", "application/ld+json")
        .header("link", context_link().as_str())
        .body(Vec::new())
        .send()
        .await
        .expect("purge");
    assert!(
        matches!(purged.status.as_u16(), 409 | 207),
        "an unsupported purge is a conflict or a keyed multi-status: {}",
        String::from_utf8_lossy(&purged.body)
    );
    assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entityOperations/purge").await,
        0,
        "no purge is forwarded on an unsupported operation"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local purge phase queries the matched entities and then deletes them all.
/// Confirms REQ-PROV-133.
#[tokio::test]
async fn purge_local_phase_queries_then_deletes() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id_one = topology.id("Vehicle:prov-purge-phase-1");
    let entity_id_two = topology.id("Vehicle:prov-purge-phase-2");
    let reg_id = topology.id("registration:prov-purge-phase");

    for entity_id in [&entity_id_one, &entity_id_two] {
        let seeded = client
            .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
            .header("content-type", "application/ld+json")
            .json(&entity(entity_id, "Vehicle", &[("speed", property(json!(42)))]))
            .send()
            .await
            .expect("seed local");
        assert_status(&seeded, 201);
    }

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("auxiliary")
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let purged = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/purge?type=Vehicle&local=true")
        .header("content-type", "application/ld+json")
        .header("link", context_link().as_str())
        .body(Vec::new())
        .send()
        .await
        .expect("purge");
    assert!(
        matches!(purged.status.as_u16(), 200 | 204 | 207),
        "the local purge phase succeeds: {}",
        String::from_utf8_lossy(&purged.body)
    );

    let first = client
        .get_entity(&entity_id_one, "local=true", "application/ld+json", None)
        .await
        .expect("first read-back");
    assert_status(&first, 404);
    let second = client
        .get_entity(&entity_id_two, "local=true", "application/ld+json", None)
        .await
        .expect("second read-back");
    assert_status(&second, 404);
    assert_eq!(mock.get_total_count().await, 0, "an auxiliary registration receives no forwarded purge");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
