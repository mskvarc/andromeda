//! Discovery-response shaping (Table 6.8.3.2-1): `pick`/`omit` (clause 4.21), `lang` (clause 4.15) applied to the returned Context Source Registration documents.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{find, query, register},
    },
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::json;
use urlencoding::encode;

/// `pick` narrows the returned registration to the named members, always keeping `id` and `type` (clause 4.21).
#[tokio::test]
async fn query_csr_pick_reduces_registration_members() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let reg_id = topology.id("registration:pick-projection");
    let body = CsrBuilder::new(&reg_id, PEER_ENDPOINT).entity_type(&vtype).build();
    register(&topology, &body).await;

    let result = query(&topology, &format!("type={}&pick=endpoint", encode(&vtype))).await;
    let registration = find(&result, &reg_id).expect("the created registration is returned");
    assert_eq!(registration["id"].as_str(), Some(reg_id.as_str()), "id survives pick");
    assert_eq!(registration["type"].as_str(), Some("ContextSourceRegistration"), "type survives pick");
    assert!(registration.get("endpoint").is_some(), "the picked member survives: {registration}");
    assert!(registration.get("information").is_none(), "an unpicked member is dropped: {registration}");

    topology.finish().await.expect("topology cleanup");
}

/// `omit` drops the named members but never `id` or `type` (clause 4.21).
#[tokio::test]
async fn query_csr_omit_removes_registration_members() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let reg_id = topology.id("registration:omit-projection");
    let body = CsrBuilder::new(&reg_id, PEER_ENDPOINT).entity_type(&vtype).build();
    register(&topology, &body).await;

    let result = query(&topology, &format!("type={}&omit=endpoint", encode(&vtype))).await;
    let registration = find(&result, &reg_id).expect("the created registration is returned");
    assert_eq!(registration["id"].as_str(), Some(reg_id.as_str()), "id survives omit");
    assert_eq!(registration["type"].as_str(), Some("ContextSourceRegistration"), "type survives omit");
    assert!(registration.get("endpoint").is_none(), "the omitted member is dropped: {registration}");

    topology.finish().await.expect("topology cleanup");
}

/// `lang` reduces a language-map member of the registration to the negotiated `Property` (clause 4.15).
#[tokio::test]
async fn query_csr_lang_reduces_languagemap() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let reg_id = topology.id("registration:lang-projection");
    let mut body = CsrBuilder::new(&reg_id, PEER_ENDPOINT).entity_type(&vtype).build();
    // A non-standard member so it lands in the registration's free-form
    // catch-all rather than a typed field; it round-trips as itself under the
    // NGSI-LD default-vocabulary context.
    body["fleetLabel"] = json!({"type": "LanguageProperty", "languageMap": {"en": "Fleet", "fr": "Flotte"}});
    register(&topology, &body).await;

    let result = query(&topology, &format!("type={}&lang=fr", encode(&vtype))).await;
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

/// A Context Source Registration query refuses `application/geo+json` (clause 6.3.4).
///
/// Clause 6.3.4 admits `GeoJSON` for "Context Information Consumption operations only, specifically 'Retrieve Entity' (see clause 5.7.1) and 'Query Entity' (clause 5.7.2)", and rules that "for all other operations, the request will result in a Not Acceptable Media Type error, returning a 406 HTTP status code". Query Context Source Registrations is clause 5.10.2, and clause 5.10.2.5 gives its output as "A JSON-LD array of matching Context Source Registrations as defined by clause 5.2.9". Clause 4.5.16 defines the `GeoJSON` representation of *Entities* and never extends it to a registration, so `geometryProperty` cannot select a Feature geometry here.
#[tokio::test]
async fn query_csr_geojson_is_not_acceptable() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let reg_id = topology.id("registration:geometry-projection");
    let mut body = CsrBuilder::new(&reg_id, PEER_ENDPOINT).entity_type(&vtype).build();
    body["location"] = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}});
    register(&topology, &body).await;

    let link = jsonld_link_header(COMPOUND_CONTEXT.as_str()).expect("valid compound-context link header");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(
            Method::GET,
            &format!("/ngsi-ld/v1/csourceRegistrations?type={}&geometryProperty=location", encode(&vtype)),
        )
        .header("accept", "application/geo+json")
        .header("link", link.to_str().expect("ascii link header"))
        .send()
        .await
        .expect("GeoJSON discovery query");
    assert_status(&response, 406);
    // The same clause fixes the body of that refusal: it "shall contain the
    // list of the available representations of the resources", which for a
    // registration resource is the two JSON representations and not the
    // GeoJSON it just refused.
    assert_eq!(
        json_body(&response),
        json!({"availableRepresentations": ["application/json", "application/ld+json"]})
    );

    topology.finish().await.expect("topology cleanup");
}
