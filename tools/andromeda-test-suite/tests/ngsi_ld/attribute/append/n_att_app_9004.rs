//! An append reports only the attributes it appended.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, partial_outcome::PartialSuccess};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{Entity, EntityFragment},
};

/// Pins: Append Attributes with `noOverwrite` naming an existing and a new attribute answers 207 whose updated list
/// holds only the new attribute, not the attribute the request did not name
/// Clause: 5.6.3.4, 5.6.3.5, 5.2.18
#[gtest]
#[tokio::test]
async fn n_att_app_9004_an_append_reports_only_the_attributes_it_appended() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_append_id = namespace.urn("Vehicle", "append")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_append = Entity::builder()
        .id(vehicle_append_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 10)
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle_append).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 99).property("colour", "red").build()?;
    let report = broker
        .entities()
        .append_attributes(&vehicle_append_id, &fragment)
        .overwrite(Overwrite::Keep)
        .send()
        .await?
        .partial()?;
    let updated = report.updated.iter().map(ToString::to_string).collect::<Vec<_>>();
    expect_that!(updated, len(eq(1)));
    expect_that!(updated.first().map(String::as_str), some(ends_with("colour")));
    Ok(())
}
