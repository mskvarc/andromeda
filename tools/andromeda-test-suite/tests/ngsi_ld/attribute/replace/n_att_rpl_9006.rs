//! Replace Attribute ignores a deletedAt written on the instance and on a sub-attribute.

use andromeda_test_suite::{
    broker,
    claimed_deletion::{has_speed_without_deletion, speed_claiming_deletion},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{ReplaceAttribute, SystemAttributes},
    },
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property},
};

/// Pins: Replace Attribute of `speed` with a Property carrying a `deletedAt` and an `accuracy` sub-attribute carrying
/// one too answers 204, and the Entity reads back, with sysAttrs, holding the written values and no `deletedAt` at
/// either depth
/// Clause: 5.2.5, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_att_rpl_9006_replace_attribute_ignores_a_written_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "replaced")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", Property::builder(50).property("accuracy", 1))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let speed_name: AttributeName = "speed".parse()?;
    let replacement = Attribute::from(Property::builder(60).property("accuracy", 2).build()?);
    // The model refuses output-only members on input, so the replacement is written as text.
    let request = broker
        .unchecked(&ReplaceAttribute::builder().id(&vehicle_id).name(&speed_name).attribute(&replacement).build())?
        .document(&UncheckedDocument::try_from(speed_claiming_deletion(60, 2)?)?);
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_speed_without_deletion(60, 2));
    Ok(())
}
