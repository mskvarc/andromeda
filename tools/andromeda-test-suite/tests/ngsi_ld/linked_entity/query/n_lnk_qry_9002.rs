//! Contained by without join level is invalid request.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with join flat and `containedBy` naming the owner answers 400 `InvalidRequest` when
/// `joinLevel` is absent, and with `joinLevel` 1 answers 200 leaving the owner out
/// Clause: 6.3.20, Table 6.4.3.2-1, 4.5.23.3
#[gtest]
#[tokio::test]
async fn n_lnk_qry_9002_contained_by_without_join_level_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let person_id = namespace.urn("Person", 1)?;
    let person_type = namespace.entity_type("Person")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let person = Entity::builder()
        .id(person_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .build()?;
    broker.entities().create(&person).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .relationship("owner", person_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Flat)
        .contained_by(vec![person_id.clone()])
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_detail_containing("containedBy")
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .contained_by(vec![person_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), not(contains(has_id(&person_id))));
    Ok(())
}
