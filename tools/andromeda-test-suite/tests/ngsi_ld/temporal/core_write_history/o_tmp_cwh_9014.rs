//! A batch upsert in update mode records the whole new instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{Entity, NgsiLdMediaType, Property},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Batch Entity Upsert with options=update and temperature 100 over temperature 25 CEL answers 204; the
/// Entity holds temperature without `unitCode`, the recorded instance holding 100 carries no `unitCode`, and q
/// selects temperature==100
/// Clause: 5.6.8.4, 5.6.2, 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9014_a_batch_upsert_in_update_mode_records_the_whole_new_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_batch_upsert_update_id = namespace.urn("Sensor", "batch-upsert-update")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_batch_upsert_update = Entity::builder()
        .id(sensor_batch_upsert_update_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(25).unit_code(UnitCode::Cel))
        .build()?;
    broker.entities().create(&sensor_batch_upsert_update).send().await?.done()?;
    let entities = [Entity::builder()
        .id(sensor_batch_upsert_update_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", 100)
        .build()?];
    broker.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_batch_upsert_update_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(100).without(Member::UnitCode)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_batch_upsert_update_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["temperature".parse()?])
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "temperature",
            all!(
                contains(any_attribute().value(100)),
                not(contains(any_attribute().value(100).with_member(Member::UnitCode)))
            )
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(100))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&sensor_batch_upsert_update_id)));
    Ok(())
}
