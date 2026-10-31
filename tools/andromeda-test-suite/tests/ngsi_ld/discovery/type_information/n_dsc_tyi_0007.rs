//! Writes that change no type or instance leave the counts.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    problem::problem,
    report_matcher::batch_error,
    type_information::families_of,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, ErrorType},
};

/// Pins: a refused create of an existing entity and a value-only update leave `entityCount` and `attributeCount` as
/// they were, and a batch create that creates one entity and refuses an existing one counts only the created one
/// Clause: 5.7.7.4, 5.7.10.4, 5.6.1.4, 5.6.2.4, 5.6.7.4, 6.14.3.1
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0007_writes_that_change_no_type_or_instance_leave_the_counts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_x_id = namespace.urn("Building", "x")?;
    let building_y_id = namespace.urn("Building", "y")?;
    let building_type = namespace.entity_type("Building")?;
    let height = namespace.attribute("height")?;
    let building_x = Entity::builder()
        .id(building_x_id.clone())
        .entity_type(building_type.clone())
        .property(namespace.attribute("height")?, 1)
        .build()?;
    broker.entities().create(&building_x).send().await?.done()?;
    let answer = broker.with_responses().entities().create(&building_x).send().await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    let fragment = EntityFragment::builder().property(namespace.attribute("height")?, 9).build()?;
    broker.entities().update_attributes(&building_x_id, &fragment).send().await?.done()?;
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information.entity_count, eq(1));
    expect_that!(information.attribute_details, len(eq(1)));
    expect_that!(families_of(&information, &height), some(len(eq(1))));
    let attribute_information = broker.with_accept(Accept::any()).attributes().retrieve(&height).send().await?;
    expect_that!(attribute_information.attribute_count, some(eq(1)));
    let entities = [
        Entity::builder()
            .id(building_x_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 1)
            .build()?,
        Entity::builder()
            .id(building_y_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 2)
            .build()?,
    ];
    let report = broker.entities().batch_create(&entities).send().await?.partial()?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_y_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&building_x_id)));
    let information_2 = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information_2.entity_count, eq(2));
    let attribute_information_2 = broker.with_accept(Accept::any()).attributes().retrieve(&height).send().await?;
    expect_that!(attribute_information_2.attribute_count, some(eq(2)));
    Ok(())
}
