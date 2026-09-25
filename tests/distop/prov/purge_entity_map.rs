//! Purge under an Entity Map (clause 5.5.14, bound to `DELETE /entities` by
//! clause 6.4.3.3).
//!
//! Table 6.4.3.3-2 makes `NGSILD-EntityMap` a request header on `DELETE
//! /entities`: "If present, the `EntityMap` supplied is used for determining the
//! set of Entities requested during the purge operation." Clause 5.5.14 (and
//! clause 5.12's last paragraph) then fix the forwarding to the map's own
//! registrations, each told the map it is linked to, exactly as a forwarded
//! query is. Without a map the purge fans out by clause 5.12 matching as before.
//!
//! `T-MOCK`: one in-process Context Source under two sub-paths, so the forwarded
//! DELETE fan-out, the `NGSILD-EntityMap` header it carries, and the narrowed
//! identifiers in its query string are each captured and asserted directly.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_status, entity_map_header},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;
use urlencoding::encode;

/// The JSON-LD `Link` header carrying the compound test context for a request
/// with no body to hold an inline `@context`.
fn context_link() -> String {
    format!(
        r#"<{}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#,
        COMPOUND_CONTEXT.as_str()
    )
}

/// A purge under a supplied `NGSILD-EntityMap` forwards a DELETE only to the
/// registrations the map names -- a registration that matches by clause 5.12 but
/// is not in the map receives none -- and each forwarded DELETE carries the
/// per-source linked-map header and the map-narrowed identifiers, not the
/// client's verbatim query.
#[tokio::test]
async fn purge_under_a_map_forwards_only_to_the_maps_registrations() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let in_map = topology.id("registration:purge-map-in");
    let out_of_map = topology.id("registration:purge-map-out");
    let entity_id = topology.id("Vehicle:purge-map");

    // The in-map source answers the query that mints the map and accepts the
    // purge; both sources also stub the DELETE so a forward to either is counted.
    mock.set_stub_with_body(
        Method::GET,
        "/in/ngsi-ld/v1/entities",
        200,
        &json!([entity(&entity_id, &vtype, &[("speed", property(json!(42)))])]),
    )
    .await;
    mock.set_stub(Method::DELETE, "/in/ngsi-ld/v1/entities", 204).await;
    mock.set_stub(Method::DELETE, "/out/ngsi-ld/v1/entities", 204).await;

    let in_registration = CsrBuilder::new(&in_map, &mock.url("/in"))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity", "purgeEntity"])
        .build();
    assert_status(&client.create_registration(&in_registration).await.expect("register in-map source"), 201);

    // Mint the map: the query records the in-map source as a holder of the entity.
    let created = client
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("mint the map");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("the mint returns a map location");

    // A second source now matches the same query by clause 5.12, but it is not
    // among the registrations the minted map names.
    let out_registration = CsrBuilder::new(&out_of_map, &mock.url("/out"))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity", "purgeEntity"])
        .build();
    assert_status(&client.create_registration(&out_registration).await.expect("register out-of-map source"), 201);

    // Clear the requests the mint produced, so only the purge's forwards remain.
    mock.drain_requests().await;

    let purged = client
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/entities?type={}", encode(&vtype)))
        .header("link", context_link().as_str())
        .header("NGSILD-EntityMap", map_id.as_str())
        .send()
        .await
        .expect("purge under the map");
    assert!(
        matches!(purged.status.as_u16(), 200 | 204 | 207),
        "a purge under a map succeeds: {}",
        String::from_utf8_lossy(&purged.body)
    );

    // Only the map's own registration is forwarded a DELETE.
    assert!(
        mock.get_count(Method::DELETE, "/in/ngsi-ld/v1/entities") >= 1,
        "the registration the map names is purged"
    );
    assert_eq!(
        mock.get_count(Method::DELETE, "/out/ngsi-ld/v1/entities"),
        0,
        "a registration matching by clause 5.12 but not in the map is not purged"
    );

    // The forwarded DELETE carries the per-source linked map and the narrowed
    // identifiers rather than the client's verbatim query.
    let forwarded = mock.drain_requests().await;
    let delete = forwarded
        .iter()
        .find(|request| request.method == Method::DELETE && request.uri.path() == "/in/ngsi-ld/v1/entities")
        .expect("the map's registration received a DELETE");
    assert!(
        delete.headers.get("ngsild-entitymap").is_some(),
        "the forwarded DELETE carries the linked-map header: {:?}",
        delete.headers
    );
    let query = delete.uri.query().unwrap_or_default();
    assert!(
        query.contains("id="),
        "the forwarded DELETE carries the map-narrowed identifiers, not the client's id-less query: {query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Control: a purge with no `NGSILD-EntityMap` header still fans out by clause
/// 5.12 matching, reaching every matching registration.
#[tokio::test]
async fn purge_without_a_map_still_fans_out_by_matching() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let first = topology.id("registration:purge-nomap-1");
    let second = topology.id("registration:purge-nomap-2");

    mock.set_stub(Method::DELETE, "/s1/ngsi-ld/v1/entities", 204).await;
    mock.set_stub(Method::DELETE, "/s2/ngsi-ld/v1/entities", 204).await;

    for (id, path) in [(&first, "/s1"), (&second, "/s2")] {
        let registration = CsrBuilder::new(id, &mock.url(path))
            .mode("redirect")
            .entity_type(&vtype)
            .id_pattern(&format!("^{}", topology.id_prefix()))
            .operations(&["purgeEntity", "retrieveEntity"])
            .build();
        assert_status(&client.create_registration(&registration).await.expect("register source"), 201);
    }

    let purged = client
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/entities?type={}", encode(&vtype)))
        .header("link", context_link().as_str())
        .send()
        .await
        .expect("purge without a map");
    assert!(
        matches!(purged.status.as_u16(), 200 | 204 | 207),
        "a purge without a map succeeds: {}",
        String::from_utf8_lossy(&purged.body)
    );

    assert!(
        mock.get_count(Method::DELETE, "/s1/ngsi-ld/v1/entities") >= 1,
        "the first matching registration is purged"
    );
    assert!(
        mock.get_count(Method::DELETE, "/s2/ngsi-ld/v1/entities") >= 1,
        "the second matching registration is purged"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
