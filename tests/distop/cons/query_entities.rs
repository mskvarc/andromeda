//! Query Entities forwarding (clause 5.7.2.4): request-width and `orderBy` validation, capability/mode-based forwarding selection, expired-entity discard, and the 4.5.5 array merge.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, typed_vehicle},
    oracle::{assert_problem_details, assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The `id` strings of every entity in an Entity Array response body.
fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// A query with none of type/attrs/q/geoQ/local is too wide and rejected with `400 BadRequestData` before any forwarding (clause 5.7.2.4).
#[tokio::test]
async fn distop_query_entities_too_wide_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    let registration_id = topology.id("registration:cons19");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let response = front.client().query_entities("", "application/ld+json", None).await.expect("query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    assert_eq!(mock.get_total_count(), 0, "a rejected too-wide query forwards nothing");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// `orderBy` is refused where a registered Context Source takes part in the answer -- clause 4.23.1's "Sort ordering is never applied to distributed operations" -- and the same `orderBy` succeeds under `local=true`, which clause 5.5.13 says leaves the Context Registry unused (clause 5.7.2.4).
#[tokio::test]
async fn distop_query_entities_orderby_with_a_participating_source_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    let registration_id = topology.id("registration:cons20");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let rejected = front
        .client()
        .query_entities(&format!("type={}&orderBy=speed", urlencoding::encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);
    assert_eq!(mock.get_total_count(), 0, "the rejected query forwards nothing");

    let accepted = front
        .client()
        .query_entities(
            &format!("type={}&orderBy=speed&local=true", urlencoding::encode(&vtype)),
            "application/ld+json",
            None,
        )
        .await
        .expect("query");
    assert_status(&accepted, 200);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-local query forwards only to `queryEntity`-capable sources; a `local=true` query forwards to none (clause 5.7.2.4).
#[tokio::test]
async fn distop_query_entities_forwards_matching_queryentity_csrs_local_none() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    for (source, operations) in [("/s1", &["queryEntity"][..]), ("/s2", &["retrieveEntity"][..])] {
        let registration_id = topology.id(&format!("registration:cons25-{}", source.trim_start_matches('/')));
        let registration = CsrBuilder::new(&registration_id, &mock.url(source))
            .mode("inclusive")
            .entity_type(&vtype)
            .operations(operations)
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/entities"), 200, &json!([]))
            .await;
    }

    assert_status(
        &front
            .client()
            .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", None)
            .await
            .expect("query"),
        200,
    );
    assert!(mock.get_count(Method::GET, "/s1/ngsi-ld/v1/entities") > 0, "queryEntity source is forwarded");
    assert_eq!(
        mock.get_count(Method::GET, "/s2/ngsi-ld/v1/entities"),
        0,
        "retrieveEntity-only source is not forwarded"
    );

    let total_before_local = mock.get_total_count();
    assert_status(
        &front
            .client()
            .query_entities(&format!("type={}&local=true", urlencoding::encode(&vtype)), "application/ld+json", None)
            .await
            .expect("query"),
        200,
    );
    assert_eq!(mock.get_total_count(), total_before_local, "a local=true query forwards to no source");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Exclusive, redirect, and inclusive sources each return an Entity Array, and the aggregated response contains entities from all three (clause 5.7.2.4). The exclusive registration names its entity id and attributes, as clause 4.3.6.3 requires ("An exclusive registration shall always relate to specific Attributes found on a single Entity. Thus, the registration shall define both: An entity id ... Attributes.").
#[tokio::test]
async fn distop_query_entities_forwards_all_modes_returns_entity_arrays() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    let mut expected = Vec::new();
    for mode in ["exclusive", "redirect", "inclusive"] {
        let source = format!("/{mode}");
        let entity_id = topology.id(&format!("Vehicle:cons29-{mode}"));
        let registration_id = topology.id(&format!("registration:cons29-{mode}"));
        let builder = CsrBuilder::new(&registration_id, &mock.url(&source))
            .mode(mode)
            .entity_type(&vtype)
            .operations(&["queryEntity"]);
        let registration = match mode {
            "exclusive" => builder.entity_id(&entity_id).property_names(&["speed"]).build(),
            "redirect" | "inclusive" => builder.build(),
            other => panic!("the mode loop names only the three proxying modes, got {other}"),
        };
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
        mock.set_stub_with_body(
            Method::GET,
            &format!("{source}/ngsi-ld/v1/entities"),
            200,
            &json!([typed_vehicle(&entity_id, &vtype, 50)]),
        )
        .await;
        expected.push(entity_id);
    }

    let response = front
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let returned = ids(&json_body(&response));
    for entity_id in &expected {
        assert!(returned.contains(entity_id), "aggregated array must contain {entity_id}, got {returned:?}");
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Entities whose attributes have all expired are discarded from a remote Entity Array before aggregation (clause 5.7.2.4).
#[tokio::test]
async fn distop_query_entities_discards_expired_entities_in_remote_array() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    let expired_id = topology.id("Vehicle:cons30-expired");
    let valid_id = topology.id("Vehicle:cons30-valid");
    let registration_id = topology.id("registration:cons30");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let expired = entity(
        &expired_id,
        &vtype,
        &[("speed", json!({ "type": "Property", "value": 90, "expiresAt": "2000-01-01T00:00:00Z" }))],
    );
    let array = json!([expired, typed_vehicle(&valid_id, &vtype, 50)]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &array).await;

    let response = front
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let returned = ids(&json_body(&response));
    assert!(returned.contains(&valid_id), "the non-expired entity is returned: {returned:?}");
    assert!(!returned.contains(&expired_id), "the fully-expired entity is discarded: {returned:?}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Register an inclusive `queryEntity` source of `entity_type` on the front broker that answers every query with `answer`, and query the front broker for `entity_type` with `extra` appended to the query string.
async fn query_answered_by_source(label: &str, answer: impl FnOnce(&str) -> Value, extra: &str) -> (Vec<Value>, String) {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let registration = CsrBuilder::new(&topology.id(&format!("registration:{label}")), &mock.url(""))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &answer(&vtype)).await;

    let response = front
        .client()
        .query_entities(&format!("type={}&{extra}", urlencoding::encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let entities = json_body(&response).as_array().cloned().unwrap_or_default();

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
    (entities, vtype)
}

/// Clause 5.7.2.4: `attrs` selects attributes by name after the merge as it does locally, so `attrs=speed` keeps `speed` and drops `maxspeed`, whose name only ends the same way.
#[tokio::test]
async fn distop_query_entities_attrs_after_merge_is_an_exact_match() {
    let id = "Vehicle:attrs-exact";
    let (entities, _) = query_answered_by_source(
        "attrs-exact",
        |vtype| {
            json!([entity(
                &format!("urn:ngsi-ld:{id}"),
                vtype,
                &[("speed", property(json!(50))), ("maxspeed", property(json!(120)))]
            )])
        },
        "attrs=speed",
    )
    .await;
    let [merged] = entities.as_slice() else {
        panic!("one entity is answered, got {entities:?}");
    };
    assert_eq!(merged["speed"]["value"], json!(50), "the named attribute is kept: {merged}");
    assert!(
        merged.get("maxspeed").is_none(),
        "an attribute that only ends like the named one is dropped: {merged}"
    );
}

/// Clause 5.7.2.4: the geo-properties a source answers are selected by `attrs` like every other attribute, so `location` does not survive the merge of a query that did not name it.
#[tokio::test]
async fn distop_query_entities_attrs_after_merge_filters_geo_properties() {
    let (entities, _) = query_answered_by_source(
        "attrs-geo",
        |vtype| {
            json!([entity(
                "urn:ngsi-ld:Vehicle:attrs-geo",
                vtype,
                &[
                    ("speed", property(json!(50))),
                    (
                        "location",
                        json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] } })
                    ),
                ]
            )])
        },
        "attrs=speed",
    )
    .await;
    let [merged] = entities.as_slice() else {
        panic!("one entity is answered, got {entities:?}");
    };
    assert_eq!(merged["speed"]["value"], json!(50), "the named attribute is kept: {merged}");
    assert!(merged.get("location").is_none(), "a geo-property the query did not name is dropped: {merged}");
}

/// Clause 5.7.2.4: an entity left with only a geo-property its query named is still an entity of the answer, not an empty one to be discarded.
#[tokio::test]
async fn distop_query_entities_an_entity_left_with_only_location_is_kept() {
    let (entities, _) = query_answered_by_source(
        "attrs-only-location",
        |vtype| {
            json!([entity(
                "urn:ngsi-ld:Vehicle:attrs-only-location",
                vtype,
                &[
                    ("speed", property(json!(50))),
                    (
                        "location",
                        json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] } })
                    ),
                ]
            )])
        },
        "attrs=location",
    )
    .await;
    let [merged] = entities.as_slice() else {
        panic!("the entity left with only its location is answered, got {entities:?}");
    };
    assert_eq!(merged["location"]["value"]["type"], json!("Point"), "the named geo-property is kept: {merged}");
    assert!(merged.get("speed").is_none(), "the attribute the query did not name is dropped: {merged}");
}

/// Per-source Entity Arrays merge per clause 4.5.5: a same-id entity is merged across sources, and distinct-id entities coexist.
#[tokio::test]
async fn distop_query_entities_merges_arrays_per_4_5_5() {
    let topology = topology::chain().await.expect("three-broker topology");
    let vtype = topology.id("Vehicle");
    let shared_id = topology.id("Vehicle:cons31-shared");
    let extra_id = topology.id("Vehicle:cons31-extra");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder_b = topology.broker(BrokerRole::B).expect("topology broker B");
    let holder_c = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front
            .client()
            .post_entity(&entity(&shared_id, &vtype, &[("fuelLevel", property(json!(70)))]))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder_b
            .client()
            .post_entity(&entity(&shared_id, &vtype, &[("speed", property(json!(50)))]))
            .with_ld_json()
            .send()
            .await
            .expect("b shared"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .post_entity(&entity(
                &shared_id,
                &vtype,
                &[(
                    "location",
                    json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] } }),
                )],
            ))
            .with_ld_json()
            .send()
            .await
            .expect("c shared"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .post_entity(&typed_vehicle(&extra_id, &vtype, 40))
            .with_ld_json()
            .send()
            .await
            .expect("c extra"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role).expect("topology broker for role");
        let registration_id = topology.id(&format!("registration:cons31-{role:?}"));
        let registration = CsrBuilder::new(&registration_id, peer.peer_url())
            .tenant(topology.tenant())
            .mode("inclusive")
            .entity_type(&vtype)
            .operations(&["queryEntity"])
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    }

    let response = front
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(ids(&body).contains(&extra_id), "the distinct-id entity is present: {body}");
    let merged = body
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"].as_str() == Some(shared_id.as_str())))
        .cloned()
        .unwrap_or(Value::Null);
    assert!(
        merged.get("speed").is_some() && merged.get("location").is_some() && merged.get("fuelLevel").is_some(),
        "the same-id entity merges all three sources: {merged}"
    );

    topology.finish().await.expect("topology cleanup");
}
