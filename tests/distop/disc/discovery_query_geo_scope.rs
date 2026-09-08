//! Query-CSR geo, Context-Source-Filter, and scope selection (clause 5.10.2
//! over the 5.12 matching rules). All `T-LOCAL`.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{contains, query, register, register_located},
    },
    registration::CsrBuilder,
    topology,
};
use serde_json::json;
use urlencoding::encode;

/// Build a `near` geo-query around a point, optionally naming a geoproperty.
fn near_query(entity_type: &str, coordinates: [f64; 2], distance: u32, geoproperty: Option<&str>) -> String {
    let coords = encode(&format!("[{},{}]", coordinates[0], coordinates[1])).into_owned();
    let georel = encode(&format!("near;maxDistance=={distance}")).into_owned();
    let mut params = format!("type={}&georel={georel}&geometry=Point&coordinates={coords}", encode(entity_type));
    if let Some(geoproperty) = geoproperty {
        params.push_str(&format!("&geoproperty={}", encode(geoproperty)));
    }
    params
}

/// A geo-query with no `geoproperty` defaults to matching against the
/// registration's `location` `GeoProperty` (clause 5.12). Confirms REQ-DISC-28.
#[tokio::test]
async fn query_csr_geoquery_defaults_to_location_geoproperty() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("GeoDefaultType");
    let id = topology.id("registration:geo-default");
    register_located(&topology, &id, &vtype, [13.404, 52.520]).await;

    let near = query(&topology, &near_query(&vtype, [13.404, 52.520], 3000, None)).await;
    assert!(contains(&near, &id), "the default location geoproperty matches a nearby query point");

    let far = query(&topology, &near_query(&vtype, [2.349, 48.853], 3000, None)).await;
    assert!(!contains(&far, &id), "a distant query point does not match the registration's location");
    topology.finish().await.expect("topology cleanup");
}

/// A geo-query matches a registration whose location is directly covered and
/// excludes one that is geographically disjoint (clause 5.12). Confirms
/// REQ-DISC-29.
#[tokio::test]
async fn query_csr_geoquery_matches_direct_and_possible_containment() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("GeoMatchType");
    let near_id = topology.id("registration:geo-near");
    let far_id = topology.id("registration:geo-far");
    register_located(&topology, &near_id, &vtype, [13.404, 52.520]).await;
    register_located(&topology, &far_id, &vtype, [2.349, 48.853]).await;

    let body = query(&topology, &near_query(&vtype, [13.404, 52.520], 3000, Some("location"))).await;
    assert!(contains(&body, &near_id), "the registration whose location is covered matches");
    assert!(!contains(&body, &far_id), "the geographically disjoint registration is excluded");
    topology.finish().await.expect("topology cleanup");
}

/// A Context Source Filter matches registrations by their own Context Source
/// Properties (clause 4.9: the filter's "target is the name of a Context Source
/// Property member of the `CSourceRegistration`, see Table 5.2.9-1"), so the
/// fixture carries `tier` as a top-level registration member rather than a
/// `contextSourceInfo` header pair: the registration whose `tier` satisfies the
/// filter matches and the other is excluded (clause 5.10.2.4). Confirms
/// REQ-DISC-37.
#[tokio::test]
async fn query_csr_csf_matched_against_source_properties() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("CsfType");
    let gold = topology.id("registration:csf-gold");
    let silver = topology.id("registration:csf-silver");
    let mut gold_body = CsrBuilder::new(&gold, PEER_ENDPOINT).entity_type(&vtype).build();
    gold_body["tier"] = json!("gold");
    let mut silver_body = CsrBuilder::new(&silver, PEER_ENDPOINT).entity_type(&vtype).build();
    silver_body["tier"] = json!("silver");
    register(&topology, &gold_body).await;
    register(&topology, &silver_body).await;

    let body = query(&topology, &format!("type={}&csf={}", encode(&vtype), encode("tier==gold"))).await;
    assert!(contains(&body, &gold), "the registration satisfying the context source filter matches");
    assert!(!contains(&body, &silver), "the registration failing the context source filter is excluded");
    topology.finish().await.expect("topology cleanup");
}

/// A scope query matches registrations against their registered scope:
/// `/Madrid/#` matches a Madrid-scoped registration and excludes one scoped
/// elsewhere (clause 5.12). Confirms REQ-DISC-38.
#[tokio::test]
async fn query_csr_scope_query_matched_against_scope() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("ScopeType");
    let madrid = topology.id("registration:scope-madrid");
    let berlin = topology.id("registration:scope-berlin");
    let mut madrid_body = CsrBuilder::new(&madrid, PEER_ENDPOINT).entity_type(&vtype).build();
    madrid_body["scope"] = json!(["/Madrid/Gardens"]);
    let mut berlin_body = CsrBuilder::new(&berlin, PEER_ENDPOINT).entity_type(&vtype).build();
    berlin_body["scope"] = json!(["/Berlin/Parks"]);
    register(&topology, &madrid_body).await;
    register(&topology, &berlin_body).await;

    let body = query(&topology, &format!("type={}&scopeQ={}", encode(&vtype), encode("/Madrid/#"))).await;
    assert!(contains(&body, &madrid), "the Madrid-scoped registration matches /Madrid/#");
    assert!(!contains(&body, &berlin), "the Berlin-scoped registration is excluded");
    topology.finish().await.expect("topology cleanup");
}
