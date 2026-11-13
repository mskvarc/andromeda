//! Batch Entity Upsert ignores a deletedAt written on an instance and on a sub-attribute.

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
        operation::{BatchUpsert, SystemAttributes},
    },
    model::{Entity, Lossless, NgsiLdMediaType, Property},
};

/// Pins: Batch Entity Upsert of a new Entity and of a stored one, each holding `speed` with a `deletedAt` on its
/// instance and on its `accuracy` sub-attribute, answers 201, and both Entities read back, with sysAttrs, holding the
/// written values and no `deletedAt` at either depth
/// Clause: 5.2.5, 5.6.8.4
#[gtest]
#[tokio::test]
async fn n_bat_ups_9021_batch_upsert_ignores_a_written_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let created_id = namespace.urn("Vehicle", "upserted-new")?;
    let stored_id = namespace.urn("Vehicle", "upserted-stored")?;
    let stored = Entity::builder()
        .id(stored_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).property("accuracy", 1))
        .build()?;
    broker.entities().create(&stored).send().await?.done()?;
    let created = Entity::builder().id(created_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses output-only members on input, so `speed` is written as text.
    let speed = speed_claiming_deletion(60, 2)?;
    let documents = [
        UncheckedDocument::from_entity(&created, Lossless::Normalized).set_member("speed", &speed),
        UncheckedDocument::from_entity(&stored, Lossless::Normalized).set_member("speed", &speed),
    ];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&[created, stored]).build())?
        .documents(&documents);
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::CREATED));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for id in [&created_id, &stored_id] {
        let retrieved = json.entities().retrieve(id).system_attributes(SystemAttributes::Include).send().await?;
        expect_that!(retrieved.body(), has_speed_without_deletion(60, 2), "{id}");
    }
    Ok(())
}
