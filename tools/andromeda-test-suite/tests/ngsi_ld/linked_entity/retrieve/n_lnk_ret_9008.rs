//! Contained by applies only with join level.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join flat and `containedBy` naming the owner answers 400 `InvalidRequest` when
/// `joinLevel` is absent, and leaves the owner out when `joinLevel` is 1
/// Clause: Table 6.5.3.1-1, 5.7.1.4, 4.5.23.3, 6.3.20
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9008_contained_by_applies_only_with_join_level() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_r12_id = namespace.urn("Company", "r12")?;
    let company_type = namespace.entity_type("Company")?;
    let person_r12_id = namespace.urn("Person", "r12")?;
    let person_type = namespace.entity_type("Person")?;
    let city_r12_id = namespace.urn("City", "r12")?;
    let city_type = namespace.entity_type("City")?;
    let building_r12_id = namespace.urn("Building", "r12")?;
    let building_type = namespace.entity_type("Building")?;
    let company_r12 = Entity::builder()
        .id(company_r12_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company_r12).send().await?.done()?;
    let person_r12 = Entity::builder()
        .id(person_r12_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_r12_id.clone())
        .build()?;
    broker.entities().create(&person_r12).send().await?.done()?;
    let city_r12 = Entity::builder()
        .id(city_r12_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .build()?;
    broker.entities().create(&city_r12).send().await?.done()?;
    let building_r12 = Entity::builder()
        .id(building_r12_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("owner", person_r12_id.clone())
        .relationship("tenant", person_r12_id.clone())
        .relationship("locatedIn", city_r12_id.clone())
        .build()?;
    broker.entities().create(&building_r12).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_r12_id)
        .contained_by(vec![person_r12_id.clone()])
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_detail_containing("containedBy")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_r12_id)
        .join_level(1.try_into()?)
        .contained_by(vec![person_r12_id.clone()])
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), not(contains(has_id(&person_r12_id))));
    Ok(())
}
