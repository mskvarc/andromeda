//! Whitespace between the parts of a q expression is skipped.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    raw_call::{call, target},
    unchecked_page::entities,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, Response, StatusCode};
use ngsi_ld::model::{Entity, EntityId, EntityTypeName, Property};

/// Checks that the answer is a 200 holding exactly the entities `ids`.
fn expect_exactly(response: &Response<Bytes>, ids: &[&EntityId]) -> Result<()> {
    verify_that!(response.status(), eq(StatusCode::OK))?;
    let found = entities(response)?;
    expect_that!(found, len(eq(ids.len())));
    for id in ids {
        expect_that!(found, contains(has_id(*id)));
    }
    Ok(())
}

/// Query Entities by GET for the type `car_type` with the query text `q`, as written.
async fn get_cars(broker: &BrokerClient, car_type: &EntityTypeName, q: &str) -> Result<Response<Bytes>> {
    // The query language of the model refuses the spaces, so the text is sent as written.
    let car_type_text = car_type.to_string();
    Ok(call(
        broker,
        Method::GET,
        &target("entities", &[("type", &car_type_text), ("q", q)]),
        &[("Accept", "application/json")],
        None,
    )
    .await?)
}

/// Pins: spaces, tabs and line breaks around the names, operators, values, list commas, connectives and parentheses of
/// a q expression are skipped, by GET and by a POST query, and the expression selects what its unspaced form does
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 6.23.3.1
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9065_whitespace_between_the_parts_of_a_q_expression_is_skipped() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let fast_red_id = namespace.urn("Car", "fast-red")?;
    let slow_red_id = namespace.urn("Car", "slow-red")?;
    let reversing_green_id = namespace.urn("Car", "reversing-green")?;
    let car_type = namespace.entity_type("Car")?;
    let entities = vec![
        Entity::builder()
            .id(fast_red_id.clone())
            .entity_type(car_type.clone())
            .property("colour", Property::builder("red"))
            .property("speed", Property::builder(20))
            .build()?,
        Entity::builder()
            .id(slow_red_id.clone())
            .entity_type(car_type.clone())
            .property("colour", Property::builder("red"))
            .property("speed", Property::builder(5))
            .build()?,
        Entity::builder()
            .id(reversing_green_id.clone())
            .entity_type(car_type.clone())
            .property("colour", Property::builder("green"))
            .property("speed", Property::builder(-1))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let spaced = get_cars(&broker, &car_type, r#"( colour == "red" , "blue" ; speed > 10 ) | speed < 0"#).await?;
    expect_exactly(&spaced, &[&fast_red_id, &reversing_green_id])?;
    let unspaced = get_cars(&broker, &car_type, r#"(colour=="red","blue";speed>10)|speed<0"#).await?;
    expect_exactly(&unspaced, &[&fast_red_id, &reversing_green_id])?;
    let body = format!(r#"{{"type": "Query", "entities": [{{"type": "{car_type}"}}], "q": "\tcolour\t==\t\"red\"\n;\nspeed >= 5\n"}}"#);
    let posted = call(&broker, Method::POST, "entityOperations/query", &[("Accept", "application/json")], Some(&body)).await?;
    expect_exactly(&posted, &[&fast_red_id, &slow_red_id])?;
    let quoted = get_cars(&broker, &car_type, r#"colour == " red""#).await?;
    expect_exactly(&quoted, &[])?;
    Ok(())
}
