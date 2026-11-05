//! A batch upsert reaching an entity after a later append writes it after the append.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::has_attribute,
    eventually::{Poll, delivery_deadline, eventually},
    evolution_matcher::has_evolution,
    instance_time::instance_time_where,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{SystemAttributes, UpsertMode},
    },
    model::{AttributeName, Entity, EntityFragment, NgsiLdMediaType},
    query::temporal_query::{self, TemporalProperty},
};

/// The Sensors the batch writes before the contested one: with the contested Sensor, nine chunks of the default 512,
/// four written at once, so the chunk holding the contested Sensor starts only once four chunks after the first have
/// been written.
const FILLERS: u32 = 4607;

/// Pins: a Batch Entity Upsert with options=update of 4,608 Sensors whose last is an existing Sensor holding level
/// 0, and an Append Entity Attributes of that Sensor setting level 2 and alarm "on", sent once the batch wrote its
/// first Sensor and answered 204, leave the Sensor's modifiedAt at or after the appended level's and past it when
/// the batch wrote last, every attribute modified no earlier than it was created, and the latest level of its
/// Temporal Evolution on modifiedAt and on createdAt the one the Sensor holds
/// Clause: 4.8, 5.5.11.0, 5.6.8.4, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_bat_ups_9019_a_batch_upsert_reaching_an_entity_after_a_later_append_writes_it_after_the_append() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let contested_id = namespace.urn("Sensor", "x")?;
    let contested = Entity::builder()
        .id(contested_id.clone())
        .entity_type(sensor_type.clone())
        .property("level", 0)
        .build()?;
    broker.entities().create(&contested).send().await?.done()?;
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
            .property("level", 1)
            .property("alarm", "off")
            .build()?,
    );
    let entities = entities;
    let first_id = namespace.urn("Sensor", 0)?;
    let appended = EntityFragment::builder().property("level", 2).property("alarm", "on").build()?;
    let responding = broker.with_responses();
    let writer = broker.entities();
    let (upserted, append) = tokio::join!(writer.batch_upsert(&entities).mode(UpsertMode::Update).send(), async {
        eventually(delivery_deadline(), || async {
            match responding.entities().retrieve(&first_id).send().await.answered_status() {
                Some(StatusCode::OK) => Poll::Ready(()),
                status => Poll::Pending(format!("the first Sensor answered {status:?}")),
            }
        })
        .await?;
        broker.entities().append_attributes(&contested_id, &appended).send().await?.done()?;
        Result::<()>::Ok(())
    });
    upserted?.done()?;
    append?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&contested_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let modified_at = *system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    for name in ["level", "alarm"] {
        let created_at = *system_time::of_attribute(entity, &[name], SystemMember::CreatedAt)?;
        expect_that!(*system_time::of_attribute(entity, &[name], SystemMember::ModifiedAt)?, ge(created_at));
    }
    let level: AttributeName = "level".parse()?;
    let history = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&contested_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .attrs(vec!["level".parse()?])
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let appended_at = instance_time_where(history.body(), &level, property().value(2), SystemMember::ModifiedAt)?;
    let batch_wrote_last = has_attribute("level", any_attribute().value(1)).matches(entity).is_match();
    if batch_wrote_last {
        expect_that!(modified_at, gt(appended_at));
    } else {
        expect_that!(entity, has_attribute("level", any_attribute().value(2)));
        expect_that!(modified_at, ge(appended_at));
    }
    let held = if batch_wrote_last { 1 } else { 2 };
    for time_property in [TemporalProperty::ModifiedAt, TemporalProperty::CreatedAt] {
        let latest = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve(&contested_id)
            .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?).time_property(time_property).last_n(1))
            .attrs(vec!["level".parse()?])
            .send()
            .await?;
        expect_that!(latest.body(), has_evolution("level", all!(len(eq(1)), nth(0, property().value(held)))));
    }
    Ok(())
}
