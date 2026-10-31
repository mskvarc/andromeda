//! An attrs list the entity holds none of is not found.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
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

/// Pins: Retrieve Entity with attrs=unknown answers 404 `ResourceNotFound`, and with a pick or an omit leaving one
/// member answers 200 with that member
/// Clause: 5.7.1.4, 4.21
#[gtest]
#[tokio::test]
async fn n_ent_ret_9022_an_attrs_list_the_entity_holds_none_of_is_not_found() -> Result<()> {
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
        .retrieve(&building_empty_projection_id)
        .attrs(vec!["unknown".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_empty_projection_id)
        .pick("id".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, some(anything()));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_empty_projection_id)
        .pick("type".parse::<Projection>()?)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.entity_type, some(anything()));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_empty_projection_id)
        .pick("unknown,id".parse::<Projection>()?)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.id, some(anything()));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_empty_projection_id)
        .pick("location".parse::<Projection>()?)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4, has_attribute("location", any_attribute()));
    let retrieved_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_empty_projection_id)
        .omit("id,type,name,linkedTo".parse::<Projection>()?)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(entity_5, has_attribute("location", any_attribute()));
    Ok(())
}
