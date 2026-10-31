//! A batch update refuses the deletion null.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::PartialSuccess,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, EntityFragment, ErrorType, NgsiLdMediaType, Value},
};

/// Pins: Batch Entity Update carrying urn:ngsi-ld:null as a member, or as a Property value, answers 207 with one
/// `BadRequestData` error each and leaves both attributes as they were
/// Clause: 5.6.9.4, 5.6.3, 5.5.4, 5.2.1
#[gtest]
#[tokio::test]
async fn n_bat_upd_9008_a_batch_update_refuses_the_deletion_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_update_null_id = namespace.urn("Building", "batch-update-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_update_null = Entity::builder()
        .id(building_batch_update_null_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .property("occupancy", 12)
        .build()?;
    broker.entities().create(&building_batch_update_null).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_batch_update_null_id.clone(), building_type.clone())
        .delete("temperature")
        .build()?];
    let report = broker.entities().batch_update(&fragments).send().await?.partial()?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, each(batch_error().problem(ErrorType::BadRequestData)));
    let fragments_2 = [EntityFragment::for_entity(building_batch_update_null_id.clone(), building_type.clone())
        .delete_kind("occupancy", AttributeKind::Property)
        .build()?];
    let report_2 = broker.entities().batch_update(&fragments_2).send().await?.partial()?;
    expect_that!(&report_2.success, len(eq(0)));
    expect_that!(&report_2.errors, len(eq(1)));
    expect_that!(&report_2.errors, each(batch_error().problem(ErrorType::BadRequestData)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_update_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(Value::Number("21.5".parse()?))));
    expect_that!(entity, has_attribute("occupancy", any_attribute().value(12)));
    Ok(())
}
