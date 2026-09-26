//! `splitEntities` candidate retention and page pruning (clause 5.5.9.3).
//!
//! Under `splitEntities` an entity's attributes are spread across sources, so no
//! single reader can decide whether it matches: `q`, the geoquery and `scopeQ`
//! are held back and evaluated once over the merged result. Two consequences the
//! broker used to get wrong:
//!
//! - When an `EntityMap` is minted, a locally held partial entity that fails the
//!   filter on its own must still be a candidate, because a peer may hold the
//!   part that makes it match (mint-time candidate retention).
//! - When a map is reused, an entity that no longer matches once the merged
//!   result is filtered must be shed from the map, not kept because a source
//!   still returned its fragment (page pruning).
//!
//! Topology `T-SPLIT` (A front, B supporting source), each holding part of the
//! same entity. Concrete design in `docs/distributed-tests/map.md`.

use crate::{
    entity::{attribute_fragment, entity, property},
    map::entity_map_request::create_entity_map,
    oracle::{assert_status, entity_map_header, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use serde_json::{Value, json};

/// Register `source` as an inclusive `Vehicle` source on the front broker,
/// holding `property_name` for the split entity.
async fn register_source(topology: &MultiBrokerTopology<BrokerRole>, source: BrokerRole, entity_type: &str, registration_local: &str, property_name: &str) {
    let peer = topology.broker(source).expect("topology source broker");
    let registration = CsrBuilder::new(&topology.id(registration_local), peer.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(entity_type)
        .property_names(&[property_name])
        .operations(&["queryEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
}

/// Create one `Vehicle` carrying `attributes` on `role`.
async fn seed(topology: &MultiBrokerTopology<BrokerRole>, role: BrokerRole, entity_type: &str, id: &str, attributes: &[(&str, Value)]) {
    assert_status(
        &topology
            .broker(role)
            .expect("topology broker")
            .client()
            .post_entity(&entity(id, entity_type, attributes))
            .with_ld_json()
            .send()
            .await
            .expect("seed entity"),
        201,
    );
}

/// The holder list a map records for `entity_id`, or an empty list when the map
/// holds no such candidate.
fn holders(map_body: &Value, entity_id: &str) -> Vec<String> {
    map_body["entityMap"]
        .get(entity_id)
        .and_then(Value::as_array)
        .map(|holders| holders.iter().filter_map(|holder| holder.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// Clause 5.5.9.3: a locally held partial entity that fails `q` on its own is
/// still a candidate in a minted split map, because a peer holds the part that
/// makes it match. The front broker holds only `fuel`, so `q=speed>50` cannot be
/// decided there; deferring the filter keeps it as a `@none` candidate.
#[tokio::test]
async fn distop_split_pruning_mint_retains_a_locally_partial_candidate() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let entity_id = topology.id("Vehicle:split-mint-partial");
    let vtype = topology.id("Vehicle");

    // The front broker holds only `fuel`; the supporting source holds `speed`.
    seed(&topology, BrokerRole::A, &vtype, &entity_id, &[("fuel", property(json!(30)))]).await;
    seed(&topology, BrokerRole::B, &vtype, &entity_id, &[("speed", property(json!(60)))]).await;
    register_source(&topology, BrokerRole::B, &vtype, "registration:split-mint-speed", "speed").await;

    let created = create_entity_map(
        front.client(),
        &format!("type={}&id={entity_id}&q=speed%3E50&splitEntities=true", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    let map = json_body(&created);
    let recorded = holders(&map, &entity_id);
    assert!(
        recorded.iter().any(|holder| holder == "@none"),
        "the locally held partial entity is retained as a candidate despite failing q on the front broker: {map}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// Clause 5.5.9.3: when a split map is reused, an entity that no longer matches
/// once the merged result is filtered is shed from the map, while one that still
/// matches stays. The front broker holds `speed`; lowering one entity's speed
/// below the threshold must remove it from the map on the next page.
#[tokio::test]
async fn distop_split_pruning_sheds_an_entity_that_stopped_matching_after_merge() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let stays = topology.id("Vehicle:split-prune-stays");
    let sheds = topology.id("Vehicle:split-prune-sheds");
    let vtype = topology.id("Vehicle");

    // The front broker holds `speed`, the supporting source holds `fuel`; both
    // entities match `q=speed>50` at first.
    seed(&topology, BrokerRole::A, &vtype, &stays, &[("speed", property(json!(60)))]).await;
    seed(&topology, BrokerRole::A, &vtype, &sheds, &[("speed", property(json!(70)))]).await;
    seed(&topology, BrokerRole::B, &vtype, &stays, &[("fuel", property(json!(30)))]).await;
    seed(&topology, BrokerRole::B, &vtype, &sheds, &[("fuel", property(json!(40)))]).await;
    register_source(&topology, BrokerRole::B, &vtype, "registration:split-prune-fuel", "fuel").await;

    // Mint a split map: both entities are candidates and both match.
    let created = front
        .client()
        .query_entities(
            &format!("type={}&q=speed%3E50&splitEntities=true&entityMap=true", urlencoding::encode(&vtype)),
            "application/ld+json",
            None,
        )
        .await
        .expect("mint split map");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    // Lower one entity's speed below the threshold on the front broker, so the
    // merged entity no longer matches `q=speed>50`.
    let patch = attribute_fragment(&[("speed", property(json!(10)))]);
    assert_status(&front.client().update_entity_attrs(&sheds, &patch).await.expect("lower the speed"), 204);

    // Reuse the map with the filter applied over the merged result.
    let reused = front
        .client()
        .raw(
            Method::GET,
            &format!("/ngsi-ld/v1/entities?type={}&q=speed%3E50&splitEntities=true", urlencoding::encode(&vtype)),
        )
        .header("NGSILD-EntityMap", map_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("reuse the map");
    assert_status(&reused, 200);
    let returned: Vec<String> = json_body(&reused)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    assert!(returned.contains(&stays), "the still-matching entity is returned: {returned:?}");
    assert!(!returned.contains(&sheds), "the no-longer-matching entity is excluded: {returned:?}");

    // The map itself sheds the candidate that stopped matching, and keeps the
    // one that still does.
    let map = json_body(&front.client().retrieve_entity_map(&map_id).await.expect("retrieve the reused map"));
    assert!(
        holders(&map, &stays).iter().any(|holder| holder == "@none"),
        "the still-matching entity stays in the map: {map}"
    );
    assert!(
        map["entityMap"].get(&sheds).is_none(),
        "the no-longer-matching entity is shed from the map: {map}"
    );

    topology.finish().await.expect("topology cleanup");
}
