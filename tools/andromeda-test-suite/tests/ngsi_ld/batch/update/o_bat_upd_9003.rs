//! No overwrite splits success and error per entity.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, partial_outcome::PartialSuccess, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{Entity, EntityFragment, ErrorType},
};

/// Pins: Batch Entity Update with `options=noOverwrite` of a fragment appending a new Attribute to one entity and a
/// fragment naming only held Attributes of another answers 207, the first in success and the second in errors as a
/// 400 `BadRequestData` explaining nothing was appended
/// Clause: 5.6.9.4, 5.6.9.5, 5.6.3.4
#[gtest]
#[tokio::test]
async fn o_bat_upd_9003_no_overwrite_splits_success_and_error_per_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let building_2 = Entity::builder()
        .id(building_2_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Louvre")
        .property("subCategory", "tourism")
        .build()?;
    broker.entities().create(&building_2).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_id.clone(), building_type.clone())
            .property("name", "Tour Eiffel")
            .relationship("locatedAt", city_paris_id)
            .build()?,
        EntityFragment::for_entity(building_2_id.clone(), building_type.clone())
            .property("name", "Musee du Louvre")
            .property("subCategory", "tourism")
            .build()?,
    ];
    let report = broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.partial()?;
    expect_that!(&report.success, contains(eq(&building_id)));
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&building_2_id)
                .problem(ErrorType::BadRequestData)
                .status(StatusCode::BAD_REQUEST)
                .with_detail("no attributes were appended: every named attribute already exists and noOverwrite was set")
        )
    );
    Ok(())
}
