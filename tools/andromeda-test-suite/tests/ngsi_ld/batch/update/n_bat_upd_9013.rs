//! A no overwrite entity appending nothing is listed only in errors.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Update with options=noOverwrite of a fragment naming only held Attributes beside a fragment
/// appending a new one answers 207, listing the first entity only in errors and the second only in success
/// Clause: 5.2.16, 5.6.9.4, 5.6.9.5
#[gtest]
#[tokio::test]
async fn n_bat_upd_9013_a_no_overwrite_entity_appending_nothing_is_listed_only_in_errors() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_held_id = namespace.urn("Building", "held")?;
    let building_type = namespace.entity_type("Building")?;
    let building_appended_id = namespace.urn("Building", "appended")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let entities = [
        Entity::builder()
            .id(building_held_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
        Entity::builder()
            .id(building_appended_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Louvre")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_held_id.clone(), building_type.clone())
            .property("name", "Tour Eiffel")
            .build()?,
        EntityFragment::for_entity(building_appended_id.clone(), building_type.clone())
            .relationship("locatedAt", city_paris_id)
            .build()?,
    ];
    let report = broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.partial()?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_appended_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(0, batch_error().entity(&building_held_id).problem(ErrorType::BadRequestData))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_held_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    Ok(())
}
