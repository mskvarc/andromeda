//! Purge entities refuses the query only parameters.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Purge Entities with pick, `orderBy`, format, count or lang answers 400 `InvalidRequest` and purges
/// nothing, while type with drop of speed answers 204 and removes speed
/// Clause: 6.3.20, Table 6.4.3.3-1, 5.6.21.3, 5.6.21.4
#[gtest]
#[tokio::test]
async fn n_ent_prg_9012_purge_entities_refuses_the_query_only_parameters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // Purge defines none of these query parameters, so each request is sent as written.
    let request = request_to(&broker, Method::DELETE, &format!("entities?type=Vehicle_{namespace}&pick=speed"))?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let request_2 = request_to(&broker, Method::DELETE, &format!("entities?type=Vehicle_{namespace}&orderBy=id"))?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let request_3 = request_to(&broker, Method::DELETE, &format!("entities?type=Vehicle_{namespace}&format=keyValues"))?;
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let request_4 = request_to(&broker, Method::DELETE, &format!("entities?type=Vehicle_{namespace}&count=true"))?;
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let request_5 = request_to(&broker, Method::DELETE, &format!("entities?type=Vehicle_{namespace}&lang=en"))?;
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(50)));
    let received = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(&vehicle_type)
        .drop(vec!["speed".parse::<AttributeName>()?])
        .send()
        .await?;
    received.outcome.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, lacks_attribute("speed"));
    expect_that!(entity_2, has_attribute("brandName", property().value("Mercedes")));
    Ok(())
}
