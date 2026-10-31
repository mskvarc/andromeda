//! A pick or omit leaving no member is not found.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick=unknown, pick=unknown,alsoUnknown or an omit naming every member the Entity
/// holds answers 404 `ResourceNotFound`
/// Clause: 5.7.1.4, 4.21
/// Recorded: 5.7.1.4 raises `ResourceNotFound` when the Entity holds none of the Attributes in attrs, which Table
/// 6.5.3.1-1 makes a synonym for pick; whether a pick or an omit that leaves the Entity no member is the same
/// failure is left open. Andromeda answers 404 `ResourceNotFound`, on the temporal retrieval too, while a query
/// leaves such an Entity out
#[gtest]
#[tokio::test]
async fn o_ent_ret_9023_a_pick_or_omit_leaving_no_member_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_empty_projection_id = namespace.urn("Building", "empty-projection")?;
    let building_type = namespace.entity_type("Building")?;
    let city_empty_projection_id = namespace.urn("City", "empty-projection")?;
    let building_empty_projection = Entity::builder()
        .id(building_empty_projection_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("linkedTo", city_empty_projection_id)
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building_empty_projection).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_empty_projection_id)
        .pick("unknown".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_empty_projection_id)
        .pick("unknown,alsoUnknown".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_empty_projection_id)
        .omit("id,type,name,linkedTo,location".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
