//! Federation architecture and discovery acceptance cases.

use crate::fixtures::{
    FedRole,
    assert_complete_catalog,
    assert_status,
    body,
    broker,
    discovered_client,
    entity,
    entity_with_scope,
    registration,
    registration_for_entity,
    topology,
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use pretty_assertions::assert_eq;
use serde_json::json;
use urlencoding::encode;

#[tokio::test]
async fn fed_topology_reports_distinct_identities_and_isolated_stores() {
    assert_complete_catalog();
    let topology = topology().await;
    let btype = topology.id("Building");
    let roles = [FedRole::Hub, FedRole::Madrid, FedRole::Berlin, FedRole::Nordic, FedRole::North];
    let mut aliases = Vec::new();
    for role in roles {
        let response = broker(&topology, role)
            .client()
            .source_identity(Some("application/ld+json"))
            .await
            .expect("source identity");
        assert_status(&response, 200);
        aliases.push(body(&response)["contextSourceAlias"].as_str().expect("source alias").to_owned());
        let id = topology.id(&format!("sentinel:{role:?}"));
        assert_status(
            &broker(&topology, role)
                .client()
                .post_entity(&entity_with_scope(&id, &btype, 1, &format!("/Europe/{role:?}")))
                .with_ld_json()
                .send()
                .await
                .expect("sentinel create"),
            201,
        );
        assert_status(
            &broker(&topology, role)
                .client()
                .get_entity(&id, "", "application/ld+json", None)
                .await
                .expect("sentinel read"),
            200,
        );
    }
    aliases.sort();
    aliases.dedup();
    assert_eq!(aliases.len(), roles.len());
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_registration_lifecycle_controls_domain_routing() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let id = topology.id("Building:lifecycle");
    assert_status(
        &madrid
            .client()
            .post_entity(&entity_with_scope(&id, &btype, 10, "/Europe/Madrid"))
            .with_ld_json()
            .send()
            .await
            .expect("domain create"),
        201,
    );
    let registration_id = topology.id("registration:madrid-lifecycle");
    let csr = registration(
        &registration_id,
        madrid.peer_url(),
        &btype,
        Some("/Europe/Madrid"),
        "inclusive",
        Some(&["queryEntity"]),
    );
    assert_status(&hub.client().create_registration(&csr).await.expect("registration create"), 201);
    let found = hub
        .client()
        .query_entities(&format!("type={}&scopeQ=%2FEurope%2FMadrid%2F%23", encode(&btype)), "application/ld+json", None)
        .await
        .expect("federated query");
    assert_status(&found, 200);
    assert!(body(&found).as_array().is_some_and(|entities| entities.iter().any(|value| value["id"] == id)));
    assert_status(
        &hub.client()
            .update_registration(&registration_id, &json!({"scope": ["/Europe/Berlin"]}), None)
            .await
            .expect("registration update"),
        204,
    );
    let updated_registration = hub
        .client()
        .retrieve_registration(&registration_id, None)
        .await
        .expect("updated registration read");
    assert_eq!(body(&updated_registration)["scope"], json!(["/Europe/Berlin"]));
    let absent = hub
        .client()
        .query_entities(&format!("type={}&scopeQ=%2FEurope%2FMadrid%2F%23", encode(&btype)), "application/ld+json", None)
        .await
        .expect("updated query");
    assert_status(&absent, 200);
    assert!(
        body(&absent).as_array().is_none_or(|entities| !entities.iter().any(|value| value["id"] == id)),
        "updated response: {}",
        body(&absent)
    );
    assert_status(&hub.client().delete_registration(&registration_id).await.expect("registration delete"), 204);
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_coarse_scope_selects_matching_domain() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    for (role, scope) in [
        (FedRole::Madrid, "/Europe/Madrid"),
        (FedRole::Berlin, "/Europe/Berlin"),
        (FedRole::Nordic, "/Europe/Nordic"),
    ] {
        let domain = broker(&topology, role);
        let id = topology.id(&format!("Building:{role:?}"));
        assert_status(
            &domain
                .client()
                .post_entity(&entity_with_scope(&id, &btype, 1, scope))
                .with_ld_json()
                .send()
                .await
                .expect("domain entity"),
            201,
        );
        let csr = registration(
            &topology.id(&format!("registration:{role:?}")),
            domain.peer_url(),
            &btype,
            Some(scope),
            "inclusive",
            Some(&["queryEntity"]),
        );
        assert_status(&hub.client().create_registration(&csr).await.expect("domain registration"), 201);
    }
    let response = hub
        .client()
        .query_entities(&format!("type={}&scopeQ=%2FEurope%2FMadrid%2F%23", encode(&btype)), "application/ld+json", None)
        .await
        .expect("scoped query");
    assert_status(&response, 200);
    let values = body(&response);
    assert_eq!(values.as_array().map(Vec::len), Some(1));
    assert!(values[0]["id"].as_str().is_some_and(|id| id.contains("Madrid")));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_geographic_coverage_selects_matching_domain() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let id = topology.id("Building:geo-madrid");
    let located = json!({"id": id, "type": btype, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [ -3.7, 40.4 ]}}, "@context": COMPOUND_CONTEXT.as_str()});
    assert_status(&madrid.client().post_entity(&located).with_ld_json().send().await.expect("geo entity"), 201);
    let csr = json!({"id": topology.id("registration:geo-madrid"), "type": "ContextSourceRegistration", "endpoint": madrid.peer_url(), "information": [{"entities": [{"type": btype}]}], "location": {"type": "Point", "coordinates": [-3.7,40.4]}, "operations": ["queryEntity"], "@context": COMPOUND_CONTEXT.as_str()});
    assert_status(&hub.client().create_registration(&csr).await.expect("geo registration"), 201);
    let response = hub
        .client()
        .query_entities(
            &format!(
                "type={}&georel=near;maxDistance==100000&geometry=Point&coordinates=%5B-3.7,40.4%5D",
                encode(&btype)
            ),
            "application/ld+json",
            None,
        )
        .await
        .expect("geo query");
    assert_status(&response, 200);
    assert!(
        body(&response)
            .as_array()
            .is_some_and(|entities| entities.iter().any(|value| value["id"] == id))
    );
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_query_merges_multi_domain_entities_and_dataset_instances() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let berlin = broker(&topology, FedRole::Berlin);
    let shared = topology.id("Building:shared");
    let first = json!({"id": shared, "type": btype, "speed": {"type": "Property", "value": 1, "datasetId": "urn:dataset:madrid"}, "@context": COMPOUND_CONTEXT.as_str()});
    let second = json!({"id": shared, "type": btype, "speed": {"type": "Property", "value": 2, "datasetId": "urn:dataset:berlin"}, "@context": COMPOUND_CONTEXT.as_str()});
    assert_status(&madrid.client().post_entity(&first).with_ld_json().send().await.expect("first fragment"), 201);
    assert_status(&berlin.client().post_entity(&second).with_ld_json().send().await.expect("second fragment"), 201);
    for (domain, role) in [(madrid, "madrid"), (berlin, "berlin")] {
        let csr = registration(
            &topology.id(&format!("registration:{role}")),
            domain.peer_url(),
            &btype,
            None,
            "inclusive",
            Some(&["queryEntity"]),
        );
        assert_status(&hub.client().create_registration(&csr).await.expect("domain registration"), 201);
    }
    let response = hub
        .client()
        .query_entities(&format!("type={}", encode(&btype)), "application/ld+json", None)
        .await
        .expect("merged query");
    assert_status(&response, 200);
    let values = body(&response);
    let merged = values
        .as_array()
        .and_then(|items| items.iter().find(|value| value["id"] == shared))
        .unwrap_or_else(|| panic!("one merged entity: {values}"));
    assert_eq!(merged["speed"].as_array().map(Vec::len), Some(2));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_query_is_transparent_to_consumer() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let id = topology.id("Building:transparent");
    assert_status(
        &madrid
            .client()
            .post_entity(&entity(&id, &btype, 3))
            .with_ld_json()
            .send()
            .await
            .expect("domain create"),
        201,
    );
    let csr = registration(
        &topology.id("registration:transparent"),
        madrid.peer_url(),
        &btype,
        None,
        "inclusive",
        Some(&["retrieveEntity"]),
    );
    assert_status(&hub.client().create_registration(&csr).await.expect("registration"), 201);
    let direct = madrid.client().get_entity(&id, "", "application/ld+json", None).await.expect("direct read");
    let federated = hub.client().get_entity(&id, "", "application/ld+json", None).await.expect("federated read");
    assert_status(&direct, 200);
    assert_status(&federated, 200);
    assert_eq!(body(&direct)["id"], body(&federated)["id"]);
    assert_eq!(body(&direct)["speed"], body(&federated)["speed"]);
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_centralized_domain_serves_local_state() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    for value in [1, 2] {
        let id = topology.id(&format!("Building:centralized:{value}"));
        assert_status(
            &madrid
                .client()
                .post_entity(&entity(&id, &btype, value))
                .with_ld_json()
                .send()
                .await
                .expect("local domain create"),
            201,
        );
    }
    let csr = registration(
        &topology.id("registration:centralized"),
        madrid.peer_url(),
        &btype,
        None,
        "inclusive",
        Some(&["queryEntity"]),
    );
    assert_status(&hub.client().create_registration(&csr).await.expect("registration"), 201);
    let response = hub
        .client()
        .query_entities(&format!("type={}", encode(&btype)), "application/ld+json", None)
        .await
        .expect("centralized query");
    assert_status(&response, 200);
    assert_eq!(body(&response).as_array().map(Vec::len), Some(2));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_distributed_domain_resolves_downstream_source() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let nordic = broker(&topology, FedRole::Nordic);
    let north = broker(&topology, FedRole::North);
    let id = topology.id("Building:north");
    assert_status(
        &north
            .client()
            .post_entity(&entity(&id, &btype, 8))
            .with_ld_json()
            .send()
            .await
            .expect("north create"),
        201,
    );
    assert_status(
        &nordic
            .client()
            .create_registration(&registration_for_entity(
                &topology.id("registration:nordic-north"),
                north.peer_url(),
                &id,
                &btype,
                "inclusive",
                Some(&["queryEntity"]),
            ))
            .await
            .expect("nested registration"),
        201,
    );
    assert_status(
        &hub.client()
            .create_registration(&registration_for_entity(
                &topology.id("registration:hub-nordic"),
                nordic.peer_url(),
                &id,
                &btype,
                "inclusive",
                Some(&["queryEntity"]),
            ))
            .await
            .expect("hub registration"),
        201,
    );
    let response = hub
        .client()
        .query_entities(&format!("type={}", encode(&btype)), "application/ld+json", None)
        .await
        .expect("nested read");
    assert_status(&response, 200);
    assert!(body(&response).as_array().is_some_and(|items| items.iter().any(|value| value["id"] == id)));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_nested_federation_excludes_back_edge() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let nordic = broker(&topology, FedRole::Nordic);
    let north = broker(&topology, FedRole::North);
    let id = topology.id("Building:back-edge");
    assert_status(
        &north
            .client()
            .post_entity(&entity(&id, &btype, 9))
            .with_ld_json()
            .send()
            .await
            .expect("north create"),
        201,
    );
    assert_status(
        &nordic
            .client()
            .create_registration(&registration(
                &topology.id("registration:nested-north"),
                north.peer_url(),
                &btype,
                None,
                "inclusive",
                Some(&["queryEntity"]),
            ))
            .await
            .expect("north registration"),
        201,
    );
    assert_status(
        &north
            .client()
            .create_registration(&registration(
                &topology.id("registration:north-hub-back-edge"),
                hub.peer_url(),
                &btype,
                None,
                "inclusive",
                Some(&["queryEntity"]),
            ))
            .await
            .expect("back edge registration"),
        201,
    );
    assert_status(
        &hub.client()
            .create_registration(&registration(
                &topology.id("registration:hub-nordic"),
                nordic.peer_url(),
                &btype,
                None,
                "inclusive",
                Some(&["queryEntity"]),
            ))
            .await
            .expect("hub registration"),
        201,
    );
    let response = hub
        .client()
        .query_entities(&format!("type={}", encode(&btype)), "application/ld+json", None)
        .await
        .expect("loop-safe read");
    assert_status(&response, 200);
    assert!(body(&response).as_array().is_some_and(|items| items.iter().any(|value| value["id"] == id)));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_direct_discovery_contacts_exact_domain_endpoint() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let id = topology.id("Building:direct-discovery");
    assert_status(
        &madrid
            .client()
            .post_entity(&entity(&id, &btype, 11))
            .with_ld_json()
            .send()
            .await
            .expect("domain create"),
        201,
    );
    let csr = registration(
        &topology.id("registration:direct-discovery"),
        madrid.peer_url(),
        &btype,
        Some("/Europe/Madrid"),
        "inclusive",
        Some(&["queryEntity"]),
    );
    assert_status(&hub.client().create_registration(&csr).await.expect("registration"), 201);
    let discovered = hub
        .client()
        .query_registrations(&format!("type={}&scopeQ=%2FEurope%2FMadrid%2F%23", encode(&btype)), None)
        .await
        .expect("discovery");
    assert_status(&discovered, 200);
    let discovered_body = body(&discovered);
    let endpoint = discovered_body
        .as_array()
        .and_then(|items| items.first())
        .and_then(|value| value["endpoint"].as_str())
        .expect("discovered endpoint");
    let direct = discovered_client(endpoint, 1072, hub.client().tenant())
        .raw(reqwest::Method::GET, &format!("/ngsi-ld/v1/entities?type={}", encode(&btype)))
        .send()
        .await
        .expect("direct domain query");
    assert_status(&direct, 200);
    assert!(body(&direct).as_array().is_some_and(|items| items.iter().any(|value| value["id"] == id)));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_single_access_point_combines_local_and_three_domains() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    for (role, name) in [(FedRole::Madrid, "madrid"), (FedRole::Berlin, "berlin"), (FedRole::Nordic, "nordic")] {
        let domain = broker(&topology, role);
        let id = topology.id(&format!("Building:{name}"));
        assert_status(
            &domain
                .client()
                .post_entity(&entity(&id, &btype, 1))
                .with_ld_json()
                .send()
                .await
                .expect("domain create"),
            201,
        );
        assert_status(
            &hub.client()
                .create_registration(&registration(
                    &topology.id(&format!("registration:{name}")),
                    domain.peer_url(),
                    &btype,
                    None,
                    "inclusive",
                    Some(&["queryEntity"]),
                ))
                .await
                .expect("registration"),
            201,
        );
    }
    let local = topology.id("Building:local");
    assert_status(
        &hub.client()
            .post_entity(&entity(&local, &btype, 1))
            .with_ld_json()
            .send()
            .await
            .expect("hub create"),
        201,
    );
    let response = hub
        .client()
        .query_entities(&format!("type={}", encode(&btype)), "application/ld+json", None)
        .await
        .expect("single access point query");
    assert_status(&response, 200);
    assert_eq!(body(&response).as_array().map(Vec::len), Some(4));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_split_entity_filters_after_cross_domain_merge() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let berlin = broker(&topology, FedRole::Berlin);
    let id = topology.id("Building:split-filter");
    let left = json!({"id": id, "type": btype, "speed": {"type": "Property", "value": 1}, "@context": COMPOUND_CONTEXT.as_str()});
    let right = json!({"id": id, "type": btype, "brandName": {"type": "Property", "value": "merged"}, "@context": COMPOUND_CONTEXT.as_str()});
    assert_status(&madrid.client().post_entity(&left).with_ld_json().send().await.expect("left fragment"), 201);
    assert_status(&berlin.client().post_entity(&right).with_ld_json().send().await.expect("right fragment"), 201);
    for (domain, name) in [(madrid, "left"), (berlin, "right")] {
        assert_status(
            &hub.client()
                .create_registration(&registration(
                    &topology.id(&format!("registration:{name}")),
                    domain.peer_url(),
                    &btype,
                    None,
                    "inclusive",
                    Some(&["queryEntity"]),
                ))
                .await
                .expect("registration"),
            201,
        );
    }
    let response = hub
        .client()
        .query_entities(
            &format!("type={}&q=brandName==%22merged%22&splitEntities=true", encode(&btype)),
            "application/ld+json",
            None,
        )
        .await
        .expect("merged filter");
    assert_status(&response, 200);
    assert_eq!(body(&response).as_array().map(Vec::len), Some(1), "split response: {}", body(&response));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_partial_read_failure_preserves_healthy_domain_results() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let id = topology.id("Building:healthy");
    assert_status(
        &madrid
            .client()
            .post_entity(&entity(&id, &btype, 12))
            .with_ld_json()
            .send()
            .await
            .expect("healthy create"),
        201,
    );
    assert_status(
        &hub.client()
            .create_registration(&registration(
                &topology.id("registration:healthy"),
                madrid.peer_url(),
                &btype,
                None,
                "inclusive",
                Some(&["queryEntity"]),
            ))
            .await
            .expect("healthy registration"),
        201,
    );
    let bad = json!({"id": topology.id("registration:failing"), "type": "ContextSourceRegistration", "endpoint": "http://127.0.0.1:1", "information": [{"entities": [{"type": btype}]}], "operations": ["queryEntity"], "@context": COMPOUND_CONTEXT.as_str()});
    assert_status(&hub.client().create_registration(&bad).await.expect("failing registration"), 201);
    let response = hub
        .client()
        .query_entities(&format!("type={}", encode(&btype)), "application/ld+json", None)
        .await
        .expect("partial query");
    assert_status(&response, 200);
    assert!(body(&response).as_array().is_some_and(|items| items.iter().any(|value| value["id"] == id)));
    assert!(response.headers.get("ngsild-warning").is_some(), "partial federation failure must be visible");
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_entity_map_stabilizes_cross_domain_pagination() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let berlin = broker(&topology, FedRole::Berlin);
    for (domain, name) in [(madrid, "madrid"), (berlin, "berlin")] {
        for value in 0..3 {
            let id = topology.id(&format!("Building:{name}:{value}"));
            assert_status(
                &domain
                    .client()
                    .post_entity(&entity(&id, &btype, value))
                    .with_ld_json()
                    .send()
                    .await
                    .expect("seed entity"),
                201,
            );
        }
        assert_status(
            &hub.client()
                .create_registration(&registration(
                    &topology.id(&format!("registration:{name}")),
                    domain.peer_url(),
                    &btype,
                    None,
                    "inclusive",
                    Some(&["queryEntity"]),
                ))
                .await
                .expect("registration"),
            201,
        );
    }
    let first = hub
        .client()
        .query_entities(
            &format!("type={}&entityMap=true&limit=2&count=true", encode(&btype)),
            "application/ld+json",
            None,
        )
        .await
        .expect("first map page");
    assert_status(&first, 201);
    assert!(first.headers.get("ngsild-results-count").is_some());
    assert!(body(&first).as_array().is_some_and(|items| items.len() <= 2));
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn fed_available_types_and_attributes_union_domain_metadata() {
    let topology = topology().await;
    let hub = broker(&topology, FedRole::Hub);
    for (role, entity_type) in [
        (FedRole::Madrid, "MadridBuilding"),
        (FedRole::Berlin, "BerlinBuilding"),
        (FedRole::Nordic, "NordicBuilding"),
    ] {
        let domain = broker(&topology, role);
        // A per-test type per domain so the tenant-wide /types and /attributes
        // union is not polluted by another concurrent test's metadata.
        let entity_type = topology.id(entity_type);
        assert_status(
            &hub.client()
                .create_registration(&registration(
                    &topology.id(&format!("registration:{entity_type}")),
                    domain.peer_url(),
                    &entity_type,
                    None,
                    "inclusive",
                    Some(&["queryEntity"]),
                ))
                .await
                .expect("metadata registration"),
            201,
        );
    }
    let types = hub.client().list_types("", None).await.expect("types union");
    let attrs = hub.client().list_attributes("", None).await.expect("attributes union");
    assert_status(&types, 200);
    assert_status(&attrs, 200);
    assert!(body(&types).is_object() || body(&types).is_array());
    assert!(body(&attrs).is_object() || body(&attrs).is_array());
    topology.finish().await.expect("topology cleanup");
}
