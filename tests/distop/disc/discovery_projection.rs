//! Discovery-response shaping (Table 6.8.3.2-1): `pick`/`omit` (clause 4.21),
//! `lang` (clause 4.15) and `geometryProperty` (clause 4.5.16) applied to the
//! returned Context Source Registration documents. All `T-LOCAL`: the
//! registrations are created and read back on the front broker, and each case
//! asserts the shape of the document it created, found by id, so unrelated
//! registrations under other prefixes cannot perturb it.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{find, query, register},
    },
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use reqwest::Method;
use serde_json::json;

/// `pick` narrows the returned registration to the named members, always
/// keeping `id` and `type` (clause 4.21). Confirms REQ-DISC pick projection.
#[tokio::test]
async fn query_csr_pick_reduces_registration_members() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:pick-projection");
    let body = CsrBuilder::new(&reg_id, PEER_ENDPOINT).entity_type("Vehicle").build();
    register(&topology, &body).await;

    let result = query(&topology, "type=Vehicle&pick=endpoint").await;
    let registration = find(&result, &reg_id).expect("the created registration is returned");
    assert_eq!(registration["id"].as_str(), Some(reg_id.as_str()), "id survives pick");
    assert_eq!(registration["type"].as_str(), Some("ContextSourceRegistration"), "type survives pick");
    assert!(registration.get("endpoint").is_some(), "the picked member survives: {registration}");
    assert!(registration.get("information").is_none(), "an unpicked member is dropped: {registration}");

    topology.finish().await.expect("topology cleanup");
}

/// `omit` drops the named members but never `id` or `type` (clause 4.21).
/// Confirms REQ-DISC omit projection.
#[tokio::test]
async fn query_csr_omit_removes_registration_members() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:omit-projection");
    let body = CsrBuilder::new(&reg_id, PEER_ENDPOINT).entity_type("Vehicle").build();
    register(&topology, &body).await;

    let result = query(&topology, "type=Vehicle&omit=endpoint").await;
    let registration = find(&result, &reg_id).expect("the created registration is returned");
    assert_eq!(registration["id"].as_str(), Some(reg_id.as_str()), "id survives omit");
    assert_eq!(registration["type"].as_str(), Some("ContextSourceRegistration"), "type survives omit");
    assert!(registration.get("endpoint").is_none(), "the omitted member is dropped: {registration}");

    topology.finish().await.expect("topology cleanup");
}

/// `lang` reduces a language-map member of the registration to the negotiated
/// `Property` (clause 4.15). Confirms REQ-DISC lang negotiation.
#[tokio::test]
async fn query_csr_lang_reduces_languagemap() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:lang-projection");
    let mut body = CsrBuilder::new(&reg_id, PEER_ENDPOINT).entity_type("Vehicle").build();
    // A non-standard member so it lands in the registration's free-form
    // catch-all rather than a typed field; it round-trips as itself under the
    // NGSI-LD default-vocabulary context.
    body["fleetLabel"] = json!({"type": "LanguageProperty", "languageMap": {"en": "Fleet", "fr": "Flotte"}});
    register(&topology, &body).await;

    let result = query(&topology, "type=Vehicle&lang=fr").await;
    let registration = find(&result, &reg_id).expect("the created registration is returned");
    assert_eq!(
        registration["fleetLabel"]["type"].as_str(),
        Some("Property"),
        "the language map becomes a Property: {registration}"
    );
    assert_eq!(
        registration["fleetLabel"]["value"].as_str(),
        Some("Flotte"),
        "the French entry is selected: {registration}"
    );
    assert_eq!(
        registration["fleetLabel"]["lang"].as_str(),
        Some("fr"),
        "the selected tag is reported: {registration}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// `geometryProperty` selects the named GeoProperty as the GeoJSON geometry of
/// the returned registration Feature (clause 4.5.16). Confirms REQ-DISC
/// GeoJSON geometry selection.
#[tokio::test]
async fn query_csr_geometry_property_selects_geometry() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:geometry-projection");
    let mut body = CsrBuilder::new(&reg_id, PEER_ENDPOINT).entity_type("Vehicle").build();
    body["location"] = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}});
    register(&topology, &body).await;

    let link = jsonld_link_header(COMPOUND_CONTEXT.as_str()).expect("valid compound-context link header");
    let response = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/csourceRegistrations?type=Vehicle&geometryProperty=location")
        .header("accept", "application/geo+json")
        .header("link", link.to_str().expect("ascii link header"))
        .send()
        .await
        .expect("GeoJSON discovery query");
    assert_status(&response, 200);

    let collection = json_body(&response);
    assert_eq!(
        collection["type"].as_str(),
        Some("FeatureCollection"),
        "a GeoJSON discovery answers a FeatureCollection: {collection}"
    );
    let feature = collection["features"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|feature| feature["id"].as_str() == Some(reg_id.as_str()) || feature["properties"]["id"].as_str() == Some(reg_id.as_str()))
        .unwrap_or_else(|| unreachable!("the created registration is a Feature in the collection: {collection}"));
    assert_eq!(
        feature["geometry"]["type"].as_str(),
        Some("Point"),
        "geometryProperty selects the location geometry: {feature}"
    );
    assert_eq!(
        feature["geometry"]["coordinates"],
        json!([13.4, 52.5]),
        "the selected geometry carries the location coordinates: {feature}"
    );

    topology.finish().await.expect("topology cleanup");
}
