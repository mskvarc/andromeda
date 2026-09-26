//! Federated Temporal API acceptance cases.

use crate::fixtures::{FedRole, assert_status, body, broker, topology};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use pretty_assertions::assert_eq;
use serde_json::json;
use urlencoding::encode;

fn temporal_entity(id: &str, entity_type: &str, first: i64, second: i64) -> serde_json::Value {
    json!({
        "id": id,
        "type": entity_type,
        "speed": [
            {"type": "Property", "value": first, "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": second, "observedAt": "2024-01-02T00:00:00Z"}
        ],
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// A temporal registration held by the domain broker under `tenant` (clause 4.14).
fn temporal_registration(tenant: &str, id: &str, endpoint: &str, entity_type: &str) -> serde_json::Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "tenant": tenant,
        "information": [{"entities": [{"type": entity_type}]}],
        "operations": ["queryTemporal"],
        "observationInterval": {"startAt": "2023-01-01T00:00:00Z", "endAt": "2025-01-01T00:00:00Z"},
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

#[tokio::test]
async fn fed_temporal_query_selects_domains_by_operation_and_interval() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let berlin = broker(&topology, FedRole::Berlin);
    for (domain, name) in [(madrid, "madrid"), (berlin, "berlin")] {
        let id = topology.id(&format!("Building:temporal:{name}"));
        assert_status(
            &domain
                .client()
                .create_temporal_entity(&temporal_entity(&id, &btype, 1, 2))
                .await
                .expect("temporal seed"),
            201,
        );
        assert_status(
            &hub.client()
                .create_registration(&temporal_registration(
                    topology.tenant(),
                    &topology.id(&format!("registration:temporal:{name}")),
                    domain.peer_url(),
                    &btype,
                ))
                .await
                .expect("temporal registration"),
            201,
        );
    }
    let response = hub
        .client()
        .query_temporal_entities(
            &format!(
                "timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-01-03T00:00:00Z&timeproperty=observedAt&type={}",
                encode(&btype)
            ),
            None,
        )
        .await
        .expect("federated temporal query");
    assert_status(&response, 200);
    assert!(body(&response).as_array().is_some_and(|items| items.len() >= 2));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_temporal_fragments_merge_and_apply_last_n() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let berlin = broker(&topology, FedRole::Berlin);
    let id = topology.id("Building:temporal-merged");
    assert_status(
        &madrid
            .client()
            .create_temporal_entity(&temporal_entity(&id, &btype, 1, 2))
            .await
            .expect("first temporal fragment"),
        201,
    );
    assert_status(
        &berlin
            .client()
            .create_temporal_entity(&temporal_entity(&id, &btype, 3, 4))
            .await
            .expect("second temporal fragment"),
        201,
    );
    for (domain, name) in [(madrid, "madrid"), (berlin, "berlin")] {
        assert_status(
            &hub.client()
                .create_registration(&temporal_registration(
                    topology.tenant(),
                    &topology.id(&format!("registration:temporal-merge:{name}")),
                    domain.peer_url(),
                    &btype,
                ))
                .await
                .expect("temporal registration"),
            201,
        );
    }
    let response = hub
        .client()
        .query_temporal_entities(
            &format!(
                "id={id}&type={}&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-01-03T00:00:00Z&timeproperty=observedAt&lastN=1",
                encode(&btype)
            ),
            None,
        )
        .await
        .expect("merged temporal read");
    assert_status(&response, 200);
    let response_body = body(&response);
    let entity = response_body.as_array().and_then(|items| items.first()).expect("merged temporal entity");
    assert_eq!(entity["speed"].as_array().map(Vec::len), Some(1), "merged temporal response: {response_body}");
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_nested_temporal_query_reaches_subdomain_without_backtracking() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let nordic = broker(&topology, FedRole::Nordic);
    let north = broker(&topology, FedRole::North);
    let id = topology.id("Building:nested-temporal");
    assert_status(
        &north
            .client()
            .create_temporal_entity(&temporal_entity(&id, &btype, 5, 6))
            .await
            .expect("north temporal"),
        201,
    );
    assert_status(
        &nordic
            .client()
            .create_registration(&temporal_registration(
                topology.tenant(),
                &topology.id("registration:north-temporal"),
                north.peer_url(),
                &btype,
            ))
            .await
            .expect("north temporal registration"),
        201,
    );
    assert_status(
        &north
            .client()
            .create_registration(&temporal_registration(
                topology.tenant(),
                &topology.id("registration:back-temporal"),
                hub.peer_url(),
                &btype,
            ))
            .await
            .expect("back temporal registration"),
        201,
    );
    assert_status(
        &hub.client()
            .create_registration(&temporal_registration(
                topology.tenant(),
                &topology.id("registration:nordic-temporal"),
                nordic.peer_url(),
                &btype,
            ))
            .await
            .expect("hub temporal registration"),
        201,
    );
    let response = hub
        .client()
        .query_temporal_entities(
            &format!(
                "id={id}&type={}&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-01-03T00:00:00Z&timeproperty=observedAt",
                encode(&btype)
            ),
            None,
        )
        .await
        .expect("nested temporal query");
    assert_status(&response, 200);
    assert!(body(&response).as_array().is_some_and(|items| items.iter().any(|value| value["id"] == id)));
    topology.finish().await.expect("topology cleanup");
}
