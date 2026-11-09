//! Flat projects a shared target by its first encounter.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, projected_member_count},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick id,owner{name},tenant{email} and join flat, owner and tenant naming one Person,
/// answers the Building and the Person once, holding only name
/// Clause: 5.7.1.5, 4.21, 4.5.23.3
/// Recorded: clause 5.7.1.5 appends a target unless it has been previously encountered and no clause says which
/// Relationship's projection applies when two reach it; Andromeda applies the first in the pick list and merges
/// nothing
#[gtest]
#[tokio::test]
async fn o_lnk_prj_9003_flat_projects_a_shared_target_by_its_first_encounter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_r11_id = namespace.urn("Company", "r11")?;
    let company_type = namespace.entity_type("Company")?;
    let person_r11_id = namespace.urn("Person", "r11")?;
    let person_type = namespace.entity_type("Person")?;
    let city_r11_id = namespace.urn("City", "r11")?;
    let city_type = namespace.entity_type("City")?;
    let building_r11_id = namespace.urn("Building", "r11")?;
    let building_type = namespace.entity_type("Building")?;
    let company_r11 = Entity::builder()
        .id(company_r11_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company_r11).send().await?.done()?;
    let person_r11 = Entity::builder()
        .id(person_r11_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_r11_id.clone())
        .build()?;
    broker.entities().create(&person_r11).send().await?.done()?;
    let city_r11 = Entity::builder()
        .id(city_r11_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .build()?;
    broker.entities().create(&city_r11).send().await?.done()?;
    let building_r11 = Entity::builder()
        .id(building_r11_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("owner", person_r11_id.clone())
        .relationship("tenant", person_r11_id.clone())
        .relationship("locatedIn", city_r11_id.clone())
        .build()?;
    broker.entities().create(&building_r11).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_r11_id)
        .pick("id,owner{name},tenant{email}".parse::<Projection>()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), len(eq(2)));
    expect_that!(entities.get(1).map(projected_member_count), some(eq(1)));
    expect_that!(entities.as_slice(), nth(1, has_attribute("name", any_attribute())));
    Ok(())
}
