//! A target two relationships share takes the list of its first encounter.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with join=flat over two Relationships naming one City answers it once, narrowed by the
/// group of the Relationship written first, whether the later one is bare or carries its own group
/// Clause: 4.21, 4.5.23, 5.7.1.4
/// Recorded: 5.7.1.4 appends a Linked Entity "unless a target URI has been previously encountered" without fixing
/// the encounter order of two Relationships of one Entity; Andromeda walks breadth-first in the order the members
/// were written and narrows the shared target by the first encounter alone
#[gtest]
#[tokio::test]
async fn o_lnk_prj_9011_a_target_two_relationships_share_takes_the_list_of_its_first_encounter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_first_encounter_id = namespace.urn("Building", "first-encounter")?;
    let building_type = namespace.entity_type("Building")?;
    let city_first_encounter_id = namespace.urn("City", "first-encounter")?;
    let city_type = namespace.entity_type("City")?;
    let country_first_encounter_id = namespace.urn("Country", "first-encounter")?;
    let country_type = namespace.entity_type("Country")?;
    let building_first_encounter = Entity::builder()
        .id(building_first_encounter_id.clone())
        .entity_type(building_type)
        .relationship("linkedTo", city_first_encounter_id.clone())
        .relationship("secondLinkTo", city_first_encounter_id.clone())
        .build()?;
    broker.entities().create(&building_first_encounter).send().await?.done()?;
    let city_first_encounter = Entity::builder()
        .id(city_first_encounter_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .relationship("linkedTo", country_first_encounter_id.clone())
        .build()?;
    broker.entities().create(&city_first_encounter).send().await?.done()?;
    let country_first_encounter = Entity::builder()
        .id(country_first_encounter_id.clone())
        .entity_type(country_type)
        .property("name", "France")
        .build()?;
    broker.entities().create(&country_first_encounter).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_first_encounter_id)
        .join_level(1.try_into()?)
        .pick("id,linkedTo{name},secondLinkTo".parse::<Projection>()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), len(eq(2)));
    let item = entities.as_slice().get(1).or_fail()?;
    expect_that!(item.id, none());
    expect_that!(item.entity_type, none());
    expect_that!(entities.as_slice(), nth(1, has_attribute("name", any_attribute())));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_first_encounter_id)
        .join_level(1.try_into()?)
        .pick("id,linkedTo{name},secondLinkTo{id}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body();
    let item_2 = entities_2.as_slice().get(1).or_fail()?;
    expect_that!(item_2.id, none());
    expect_that!(entities_2.as_slice(), nth(1, has_attribute("name", any_attribute())));
    Ok(())
}
