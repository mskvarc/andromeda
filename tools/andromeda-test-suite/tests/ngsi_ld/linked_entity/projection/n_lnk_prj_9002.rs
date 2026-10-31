//! An omit reaches the flat linked entities.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
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

/// Pins: Retrieve Entity with omit email and join flat answers the owner without email and with name
/// Clause: 5.7.1.5, 4.21, 4.5.23.3
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9002_an_omit_reaches_the_flat_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_r10_id = namespace.urn("Company", "r10")?;
    let company_type = namespace.entity_type("Company")?;
    let person_r10_id = namespace.urn("Person", "r10")?;
    let person_type = namespace.entity_type("Person")?;
    let city_r10_id = namespace.urn("City", "r10")?;
    let city_type = namespace.entity_type("City")?;
    let building_r10_id = namespace.urn("Building", "r10")?;
    let building_type = namespace.entity_type("Building")?;
    let company_r10 = Entity::builder()
        .id(company_r10_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company_r10).send().await?.done()?;
    let person_r10 = Entity::builder()
        .id(person_r10_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_r10_id.clone())
        .build()?;
    broker.entities().create(&person_r10).send().await?.done()?;
    let city_r10 = Entity::builder()
        .id(city_r10_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .build()?;
    broker.entities().create(&city_r10).send().await?.done()?;
    let building_r10 = Entity::builder()
        .id(building_r10_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("owner", person_r10_id.clone())
        .relationship("tenant", person_r10_id.clone())
        .relationship("locatedIn", city_r10_id.clone())
        .build()?;
    broker.entities().create(&building_r10).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_r10_id)
        .omit("email".parse::<Projection>()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(
        entities.as_slice(),
        contains(all!(has_id(&person_r10_id), has_attribute("name", any_attribute()), lacks_attribute("email")))
    );
    Ok(())
}
