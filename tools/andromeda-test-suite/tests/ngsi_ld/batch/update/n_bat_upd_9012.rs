//! Every entity and repeated instance of one batch shares one modified at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Batch Entity Update naming one Entity twice beside a second Entity answers 204, the repeated Entity holds the
/// later instance's name and both instances' additions, and both Entities and their updated name carry one modifiedAt
/// Clause: 5.5.11.0, 5.5.11.3, 5.6.9.4
#[gtest]
#[tokio::test]
async fn n_bat_upd_9012_every_entity_and_repeated_instance_of_one_batch_shares_one_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_repeated_id = namespace.urn("Building", "repeated")?;
    let building_type = namespace.entity_type("Building")?;
    let building_other_id = namespace.urn("Building", "other")?;
    let entities = [
        Entity::builder()
            .id(building_repeated_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
        Entity::builder()
            .id(building_other_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Louvre")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_repeated_id.clone(), building_type.clone())
            .property("name", "Tour Eiffel")
            .property("first", 1)
            .build()?,
        EntityFragment::for_entity(building_other_id.clone(), building_type.clone())
            .property("name", "Musee du Louvre")
            .build()?,
        EntityFragment::for_entity(building_repeated_id.clone(), building_type.clone())
            .property("name", "La Dame de Fer")
            .property("second", 2)
            .build()?,
    ];
    broker.entities().batch_update(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_repeated_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let batch_modified_at = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    expect_that!(entity, has_attribute("name", any_attribute().value("La Dame de Fer")));
    expect_that!(entity, has_attribute("first", any_attribute().value(1)));
    expect_that!(entity, has_attribute("second", any_attribute().value(2)));
    for name in ["name", "first", "second"] {
        expect_that!(system_time::of_attribute(entity, &[name], SystemMember::ModifiedAt)?, eq(batch_modified_at));
    }
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_other_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Musee du Louvre")));
    expect_that!(system_time::of_entity(entity_2, SystemMember::ModifiedAt)?, eq(batch_modified_at));
    expect_that!(system_time::of_attribute(entity_2, &["name"], SystemMember::ModifiedAt)?, eq(batch_modified_at));
    Ok(())
}
