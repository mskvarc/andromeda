//! An overwriting update replaces an attribute whole and no overwrite keeps it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::PartialSuccess,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Batch Entity Update of a name holding the sub-attribute accuracy, with a fragment that omits accuracy,
/// replaces name whole under the default overwrite so accuracy is gone, and under noOverwrite leaves name and its
/// accuracy untouched
/// Clause: 5.6.3.4, 5.6.9.3, 5.6.9.4
#[gtest]
#[tokio::test]
async fn n_bat_upd_9011_an_overwriting_update_replaces_an_attribute_whole_and_no_overwrite_keeps_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_overwrite_id = namespace.urn("Building", "overwrite")?;
    let building_type = namespace.entity_type("Building")?;
    let building_preserve_id = namespace.urn("Building", "preserve")?;
    let entities = [
        Entity::builder()
            .id(building_overwrite_id.clone())
            .entity_type(building_type.clone())
            .property("name", Property::builder("Eiffel Tower").property("accuracy", 5))
            .build()?,
        Entity::builder()
            .id(building_preserve_id.clone())
            .entity_type(building_type.clone())
            .property("name", Property::builder("Eiffel Tower").property("accuracy", 5))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_overwrite_id.clone(), building_type.clone())
        .property("name", "Tour Eiffel")
        .build()?];
    broker.entities().batch_update(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_overwrite_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("name", any_attribute().value("Tour Eiffel").without_sub_attribute("accuracy"))
    );
    let fragments_2 = [EntityFragment::for_entity(building_preserve_id.clone(), building_type.clone())
        .property("name", "Tour Eiffel")
        .build()?];
    broker
        .entities()
        .batch_update(&fragments_2)
        .overwrite(Overwrite::Keep)
        .send()
        .await?
        .partial()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_preserve_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(
            "name",
            any_attribute().value("Eiffel Tower").sub_attribute("accuracy", any_attribute().value(5))
        )
    );
    Ok(())
}
