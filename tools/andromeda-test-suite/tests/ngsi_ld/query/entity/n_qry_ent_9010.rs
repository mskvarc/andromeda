//! A posted query refuses its body members on the url.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{Entity, ErrorType};

/// Pins: Query Entities via POST with pick, omit, `datasetId`, join, `joinLevel` or lang on the URL answers 400
/// `InvalidRequest`, while pick in the body with format and local on the URL answers 200
/// Clause: 6.3.20, Table 6.23.3.1-1, Table 5.2.23-1, 6.3.7, 6.3.18
#[gtest]
#[tokio::test]
async fn n_qry_ent_9010_a_posted_query_refuses_its_body_members_on_the_url() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The members a Query document carries are refused on the URL, which the client never writes, so each call goes out as written.
    let fields = [("Content-Type", "application/json"), ("Accept", "application/json")];
    let body = format!(r#"{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}"}}]}}"#);
    let refused = [
        vec![("pick", "speed")],
        vec![("omit", "speed")],
        vec![("datasetId", "@none")],
        vec![("join", "flat"), ("joinLevel", "1")],
        vec![("lang", "en")],
    ];
    for parameters in &refused {
        let request = exchange(
            &broker,
            Method::POST,
            &target("entityOperations/query", parameters),
            &fields,
            Some(body.as_str()),
        )?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    }
    // The picked entities carry no `type`, and no page reader of the suite takes a projected simplified entity, so the
    // answer is read as received.
    let projected = format!(r#"{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}"}}], "pick": ["id", "speed"]}}"#);
    let request = exchange(
        &broker,
        Method::POST,
        &target("entityOperations/query", &[("format", "keyValues"), ("local", "true")]),
        &fields,
        Some(projected.as_str()),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let body_2 = answer?.into_body();
    expect_that!(&body_2, json_equals("$[0].id", vehicle_id.to_string()));
    expect_that!(&body_2, json_equals("$[0].speed", 50));
    expect_that!(&body_2, json_absent("$[0].type"));
    Ok(())
}
