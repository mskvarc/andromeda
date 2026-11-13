//! Batch Entity Creation ignores a deletedAt written on an instance and on a sub-attribute.

use andromeda_test_suite::{
    broker,
    claimed_deletion::{has_speed_without_deletion, speed_claiming_deletion},
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{BatchCreate, SystemAttributes},
    },
    model::{Entity, Lossless, NgsiLdMediaType},
};

/// Pins: Batch Entity Creation of an Entity holding `speed` with a `deletedAt` on its instance and on its `accuracy`
/// sub-attribute answers 201, and the Entity reads back, with sysAttrs, holding the written values and no `deletedAt`
/// at either depth
/// Clause: 5.2.5, 5.6.7.4
#[gtest]
#[tokio::test]
async fn n_bat_cre_9011_batch_creation_ignores_a_written_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "batch-created")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .build()?;
    // The model refuses output-only members on input, so `speed` is written as text.
    let documents = [UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &speed_claiming_deletion(60, 2)?)];
    let request = broker.unchecked(&BatchCreate::builder().entities(&[vehicle]).build())?.documents(&documents);
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::CREATED));
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
