//! A batch delete reaching an entity after a later write records its deletion after it.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::property,
    broker,
    deleted_instance::deleted_instance,
    eventually::{Poll, delivery_deadline, eventually},
    instance_time::instance_time_where,
    namespace::Namespace,
    outcome::Done,
    system_time::SystemMember,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, EntityFragment, EntityId, NgsiLdMediaType},
    query::temporal_query::{self, TemporalProperty},
};

/// The Sensors the batch deletes before the contested one: with the contested Sensor, nine chunks of the default
/// 512, four written at once, so the chunk holding the contested Sensor starts only once four chunks after the first
/// have been written.
const FILLERS: u32 = 4607;

/// Pins: a Batch Entity Delete of 4,608 Sensors whose last holds level 0, and an Append Entity Attributes of that
/// Sensor setting level 2, sent once the batch deleted its first Sensor and answered 204, leave the deletion of level
/// in the Sensor's Temporal Evolution later than the appended level, so no instance stays in effect after the Sensor
/// is gone
/// Clause: 4.8, 5.5.11.0, 5.6.6, 5.6.10.4
#[gtest]
#[tokio::test]
async fn n_bat_del_9005_a_batch_delete_reaching_an_entity_after_a_later_write_records_its_deletion_after_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let contested_id = namespace.urn("Sensor", "x")?;
    let mut entities = Vec::new();
    for number in 0..FILLERS {
        entities.push(
            Entity::builder()
                .id(namespace.urn("Sensor", number)?)
                .entity_type(sensor_type.clone())
                .property("level", 1)
                .build()?,
        );
    }
    entities.push(
        Entity::builder()
            .id(contested_id.clone())
            .entity_type(sensor_type)
            .property("level", 0)
            .build()?,
    );
    let entities = entities;
    broker.entities().batch_create(&entities).send().await?.done()?;
    // clone: the batch names every Sensor the creation wrote
    let ids: Vec<EntityId> = entities.iter().map(|entity| entity.id.clone()).collect();
    let first_id = namespace.urn("Sensor", 0)?;
    let appended = EntityFragment::builder().property("level", 2).build()?;
    let responding = broker.with_responses();
    let writer = broker.entities();
    let (deleted, append) = tokio::join!(writer.batch_delete(&ids).send(), async {
        eventually(delivery_deadline(), || async {
            match responding.entities().retrieve(&first_id).send().await.answered_status() {
                Some(StatusCode::NOT_FOUND) => Poll::Ready(()),
                status => Poll::Pending(format!("the first Sensor answered {status:?}")),
            }
        })
        .await?;
        Result::<Option<StatusCode>>::Ok(responding.entities().append_attributes(&contested_id, &appended).send().await.answered_status())
    });
    deleted?.done()?;
    let append = append?;
    // The append reaching the Sensor after the batch deleted it changes nothing the deletion has to come after.
    expect_that!(append, some(any![eq(StatusCode::NO_CONTENT), eq(StatusCode::NOT_FOUND)]));
    if append == Some(StatusCode::NO_CONTENT) {
        let level: AttributeName = "level".parse()?;
        let observed = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve(&contested_id)
            .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
            .attrs(vec!["level".parse()?])
            .send()
            .await?;
        let appended_at = instance_time_where(observed.body(), &level, property().value(2), SystemMember::ModifiedAt)?;
        let ended = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve(&contested_id)
            .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::DeletedAt))
            .attrs(vec!["level".parse()?])
            .send()
            .await?;
        let deleted_at = instance_time_where(ended.body(), &level, deleted_instance(), SystemMember::DeletedAt)?;
        expect_that!(deleted_at, gt(appended_at));
    }
    Ok(())
}
