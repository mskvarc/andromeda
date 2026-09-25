//! Bounded federation workload acceptance cases.

use crate::fixtures::{FedRole, assert_status, body, broker, entity, entity_with_scope, registration, topology};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use futures::future::join_all;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;
use urlencoding::encode;

async fn seed_entities(topology: &MultiBrokerTopology<FedRole>, role: FedRole, prefix: &str, entity_type: &str) {
    let domain = broker(topology, role);
    let scope = match role {
        FedRole::Madrid => Some("/Europe/Madrid"),
        FedRole::Berlin => Some("/Europe/Berlin"),
        FedRole::Nordic => Some("/Europe/Nordic"),
        FedRole::North => Some("/Europe/North"),
        FedRole::Hub => None,
    }
    .expect("stress data is seeded only in data domains");
    let entities: Vec<_> = (0..1000)
        .map(|value| entity_with_scope(&topology.id(&format!("{prefix}:{value}")), entity_type, value, scope))
        .collect();
    let response = domain.client().batch_create(&json!(entities)).await.expect("stress batch create");
    assert!(response.status.as_u16() < 500, "stress batch create returned {}", response.status);
}

// The three stress tests each drive thousands of entities across 64 clients, so
// they are serialised among themselves to avoid crowding the five-broker stack;
// their per-test type still isolates them from the light tests running around.
#[serial_test::serial(fed_stress)]
#[tokio::test]
async fn fed_stress_current_query_4000_entities_64_clients() {
    timeout(Duration::from_secs(300), async {
        let topology = topology().await;
        let btype = topology.id("Building");
        let hub = broker(&topology, FedRole::Hub);
        for (role, prefix) in [
            (FedRole::Madrid, "MAD"),
            (FedRole::Berlin, "BER"),
            (FedRole::Nordic, "NOR"),
            (FedRole::North, "NORTH"),
        ] {
            let domain = broker(&topology, role);
            assert_status(
                &hub.client()
                    .create_registration(&registration(
                        &topology.id(&format!("registration:stress:{prefix}")),
                        domain.peer_url(),
                        &btype,
                        None,
                        "inclusive",
                        Some(&["queryEntity"]),
                    ))
                    .await
                    .expect("stress registration"),
                201,
            );
            seed_entities(&topology, role, prefix, &btype).await;
        }
        let first = hub
            .client()
            .query_entities(
                &format!("type={}&entityMap=true&limit=250&count=true", encode(&btype)),
                "application/ld+json",
                None,
            )
            .await
            .expect("stress first page");
        assert_status(&first, 201);
        assert!(body(&first).as_array().is_some_and(|items| !items.is_empty() && items.len() <= 250));
        assert_eq!(first.headers.get("ngsild-results-count").and_then(|value| value.to_str().ok()), Some("4000"));
        let scopes = ["Madrid", "Berlin", "Nordic", "North"];
        let clients: Vec<_> = (0..64).map(|value| (hub.client().clone(), scopes[value % scopes.len()])).collect();
        let type_param = encode(&btype).into_owned();
        let responses = join_all(clients.into_iter().map(|(client, domain)| {
            let type_param = type_param.clone();
            async move {
                client
                    .query_entities(&format!("type={type_param}&scopeQ=%2FEurope%2F{domain}%2F%23"), "application/ld+json", None)
                    .await
                    .expect("concurrent stress query")
            }
        }))
        .await;
        assert!(
            responses.iter().all(|response| response.status.is_success()),
            "concurrent statuses: {:?}",
            responses.iter().map(|response| response.status).collect::<Vec<_>>()
        );
        topology.finish().await.expect("topology cleanup");
    })
    .await
    .expect("current federation stress completed within five minutes");
}

fn temporal_stress_entity(id: &str, entity_type: &str) -> serde_json::Value {
    let observations: Vec<_> = (0..20)
        .map(|value| json!({"type": "Property", "value": value, "observedAt": format!("2024-01-{:02}T00:00:00Z", value + 1)}))
        .collect();
    json!({"id": id, "type": entity_type, "speed": observations, "@context": COMPOUND_CONTEXT.as_str()})
}

#[serial_test::serial(fed_stress)]
#[tokio::test]
async fn fed_stress_temporal_20000_observations_64_clients() {
    timeout(Duration::from_secs(300), async {
        let topology = topology().await;
        let btype = topology.id("Building");
        let hub = broker(&topology, FedRole::Hub);
        for (role, prefix) in [(FedRole::Madrid, "MAD"), (FedRole::Berlin, "BER"), (FedRole::Nordic, "NOR"), (FedRole::North, "NORTH")] {
            let domain = broker(&topology, role);
            assert_status(&hub.client().create_registration(&json!({"id": topology.id(&format!("registration:temporal-stress:{prefix}")), "type": "ContextSourceRegistration", "endpoint": domain.peer_url(), "information": [{"entities": [{"type": btype}]}], "operations": ["queryTemporal"], "observationInterval": {"startAt": "2024-01-01T00:00:00Z", "endAt": "2025-01-01T00:00:00Z"}, "@context": COMPOUND_CONTEXT.as_str()})).await.expect("temporal stress registration"), 201);
            for value in 0..250 {
                assert_status(&domain.client().create_temporal_entity(&temporal_stress_entity(&topology.id(&format!("{prefix}:{value}")), &btype)).await.expect("temporal stress seed"), 201);
            }
        }
        let clients: Vec<_> = (0..64).map(|_| hub.client().clone()).collect();
        let query = format!("timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-02-01T00:00:00Z&type={}&lastN=5", encode(&btype));
        let responses = join_all(clients.into_iter().map(|client| {
            let query = query.clone();
            async move { client.query_temporal_entities(&query, None).await.expect("concurrent temporal query") }
        }))
        .await;
        assert!(responses.iter().all(|response| response.status.is_success()));
        topology.finish().await.expect("topology cleanup");
    })
    .await
    .expect("temporal federation stress completed within five minutes");
}

#[serial_test::serial(fed_stress)]
#[tokio::test]
async fn fed_stress_64_cross_domain_subscriptions() {
    timeout(Duration::from_secs(300), async {
        let topology = topology().await;
        let btype = topology.id("Building");
        let hub = broker(&topology, FedRole::Hub);
        let madrid = broker(&topology, FedRole::Madrid);
        assert_status(&hub.client().create_registration(&registration(&topology.id("registration:subscription-stress"), madrid.peer_url(), &btype, None, "inclusive", Some(&["queryEntity", "createSubscription", "updateSubscription", "deleteSubscription"]))).await.expect("subscription stress registration"), 201);
        let endpoint = "http://host.docker.internal:0/notifications";
        let ids: Vec<_> = (0..64).map(|value| topology.id(&format!("subscription:stress:{value}"))).collect();
        for id in &ids {
            let subscription = json!({"id": id, "type": "Subscription", "entities": [{"type": btype}], "notification": {"endpoint": {"uri": endpoint, "accept": "application/ld+json"}}, "localOnly": false, "@context": COMPOUND_CONTEXT.as_str()});
            let response = hub.client().create_subscription(&subscription).await.expect("stress subscription create");
            assert!(response.status.as_u16() < 500, "subscription create returned {}", response.status);
        }
        for value in 0..64 {
            let id = topology.id(&format!("Building:subscription-stress:{value}"));
            assert_status(&madrid.client().post_entity(&entity(&id, &btype, value)).with_ld_json().send().await.expect("stress watched entity"), 201);
        }
        let deletes = join_all(ids.iter().map(|id| hub.client().delete_subscription(id))).await;
        assert!(deletes.iter().all(|result| result.as_ref().is_ok_and(|response| response.status.as_u16() < 500)));
        topology.finish().await.expect("topology cleanup");
    })
    .await
    .expect("subscription federation stress completed within five minutes");
}
