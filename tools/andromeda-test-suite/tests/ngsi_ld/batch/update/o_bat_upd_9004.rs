//! No overwrite appending nothing is an error explaining why.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, partial_outcome::PartialSuccess, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{Entity, EntityFragment, ErrorType},
};

/// Pins: Batch Entity Update with `options=noOverwrite` of a fragment naming only held Attributes answers 207 with
/// the entity in errors as a 400 `BadRequestData` explaining nothing was appended
/// Clause: 5.6.9.4, 5.6.9.5
#[gtest]
#[tokio::test]
async fn o_bat_upd_9004_no_overwrite_appending_nothing_is_an_error_explaining_why() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_id.clone(), building_type.clone())
        .property("name", "Tour Eiffel")
        .property("subCategory", "tourism")
        .build()?];
    let report = broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.partial()?;
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&building_id)
                .problem(ErrorType::BadRequestData)
                .status(StatusCode::BAD_REQUEST)
                .with_detail("no attributes were appended: every named attribute already exists and noOverwrite was set")
        )
    );
    Ok(())
}
