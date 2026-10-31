//! A malformed entity map header is bad request data and creates no map.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, raw_call::target, unchecked_call::exchange};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::model::{Entity, ErrorType};

/// Pins: Query Entities, through GET and through POST `/entityOperations/query`, with `entityMap` true and an
/// `NGSILD-EntityMap` header that is not a URI answers 400 `BadRequestData` naming the header and creates no
/// `EntityMap`
/// Clause: Table 6.4.3.2-2, 6.23.1, 5.5.9.3
#[gtest]
#[tokio::test]
async fn n_qry_pag_9016_a_malformed_entity_map_header_is_bad_request_data_and_creates_no_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(namespace.urn("Building", 1)?).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The header names no URI, which the typed client cannot write, so each call goes out as written.
    let detail = "the ngsild-entitymap header is not a uri";
    let refusal = problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
        .with_detail(detail)
        .without_header(HeaderName::from_static("ngsild-entitymap"));
    let type_text = building_type.to_string();
    let read = target("entities", &[("type", type_text.as_str()), ("entityMap", "true")]);
    let answer = broker
        .send_unchecked(exchange(
            &broker,
            Method::GET,
            &read,
            &[("Accept", "application/json"), ("NGSILD-EntityMap", "not-a-uri")],
            None,
        )?)
        .await;
    expect_that!(&answer, refusal.clone());
    let body = format!(r#"{{"type": "Query", "entities": [{{"type": "{building_type}"}}], "entityMap": true}}"#);
    let answer_2 = broker
        .send_unchecked(exchange(
            &broker,
            Method::POST,
            "entityOperations/query",
            &[
                ("Content-Type", "application/json"),
                ("Accept", "application/json"),
                ("NGSILD-EntityMap", "not-a-uri"),
            ],
            Some(body.as_str()),
        )?)
        .await;
    expect_that!(&answer_2, refusal);
    Ok(())
}
