//! A plain pick reaches the flat linked entities.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, projected_member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick id,name,owner and join flat answers the owner holding only id and name, and no
/// City, the `locatedIn` Relationship not being returned
/// Clause: 5.7.1.5, 4.21, 4.5.23.3
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9001_a_plain_pick_reaches_the_flat_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_r9_id = namespace.urn("Company", "r9")?;
    let company_type = namespace.entity_type("Company")?;
    let person_r9_id = namespace.urn("Person", "r9")?;
    let person_type = namespace.entity_type("Person")?;
    let city_r9_id = namespace.urn("City", "r9")?;
    let city_type = namespace.entity_type("City")?;
    let building_r9_id = namespace.urn("Building", "r9")?;
    let building_type = namespace.entity_type("Building")?;
    let company_r9 = Entity::builder()
        .id(company_r9_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company_r9).send().await?.done()?;
    let person_r9 = Entity::builder()
        .id(person_r9_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_r9_id.clone())
        .build()?;
    broker.entities().create(&person_r9).send().await?.done()?;
    let city_r9 = Entity::builder()
        .id(city_r9_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .build()?;
    broker.entities().create(&city_r9).send().await?.done()?;
    let building_r9 = Entity::builder()
        .id(building_r9_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("owner", person_r9_id.clone())
        .relationship("tenant", person_r9_id.clone())
        .relationship("locatedIn", city_r9_id.clone())
        .build()?;
    broker.entities().create(&building_r9).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_r9_id)
        .pick("id,name,owner".parse::<Projection>()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(
        entities
            .iter()
            .filter(|item| item.id.as_ref() == Some(&person_r9_id))
            .map(projected_member_count)
            .sum::<usize>(),
        eq(2)
    );
    expect_that!(
        entities.as_slice(),
        contains(all!(has_id(&person_r9_id), has_attribute("name", any_attribute())))
    );
    expect_that!(entities.as_slice(), not(contains(has_id(&city_r9_id))));
    Ok(())
}
