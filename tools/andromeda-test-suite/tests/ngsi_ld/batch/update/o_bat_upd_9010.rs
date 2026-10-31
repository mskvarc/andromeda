//! No overwrite with one new attribute is a success.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Batch Entity Update with `options=noOverwrite` whose fragment names the present name and the new
/// Relationship `locatedAt` answers 204, keeps name "Eiffel Tower" and appends `locatedAt`
/// Clause: 5.6.9.4, 5.6.9.5, 5.6.3.4, 6.16.3.1
/// Recorded: clause 5.6.9.4 adds an entity to S when it "was successfully updated (Attributes appended)" without
/// saying whether an Attribute left in place by `noOverwrite` counts against that; Andromeda counts the entity as
/// updated once any Attribute is appended and answers 204
#[gtest]
#[tokio::test]
async fn o_bat_upd_9010_no_overwrite_with_one_new_attribute_is_a_success() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_partial_id = namespace.urn("Building", "partial")?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "paris")?;
    let building_partial = Entity::builder()
        .id(building_partial_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .build()?;
    broker.entities().create(&building_partial).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_partial_id.clone(), building_type.clone())
        .property("name", "Tour Eiffel")
        .relationship("locatedAt", city_paris_id.clone())
        .build()?];
    broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_partial_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("locatedAt", any_attribute().object(city_paris_id.clone())));
    Ok(())
}
