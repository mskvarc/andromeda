//! Replace Entity ignores a deletedAt written on an instance and on a sub-attribute.

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
        operation::{ReplaceEntity, SystemAttributes},
    },
    model::{Entity, Lossless, NgsiLdMediaType, Property},
};

/// Pins: Replace Entity with a body holding `speed` with a `deletedAt` on its instance and on its `accuracy`
/// sub-attribute answers 204, and the Entity reads back, with sysAttrs, holding the written values and no `deletedAt`
/// at either depth
/// Clause: 5.2.5, 5.6.18.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9011_replace_entity_ignores_a_written_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "replaced")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", Property::builder(50).property("accuracy", 1))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The model refuses output-only members on input, so `speed` is written as text; Replace Entity names its target
    // in the path only, so the body carries no `id`.
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized)
        .remove_member("id")
        .set_member("speed", &speed_claiming_deletion(60, 2)?);
    let request = broker.unchecked(&ReplaceEntity::builder().entity(&vehicle).build())?.document(&document);
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
