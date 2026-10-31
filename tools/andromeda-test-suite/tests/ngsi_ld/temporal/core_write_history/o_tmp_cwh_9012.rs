//! Batch merge records the instance with the unit code it keeps.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
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
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Batch Entity Merge with temperature 100 over temperature 25 CEL answers 204; the Entity holds 100 CEL, the
/// recorded instance holding 100 carries `unitCode` CEL, and q selects temperature==100
/// Clause: 5.6.20.4, 5.6.17, 5.5.12 (EXAMPLE 1), 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9012_batch_merge_records_the_instance_with_the_unit_code_it_keeps() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_batch_merge_id = namespace.urn("Sensor", "batch-merge")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_batch_merge = Entity::builder()
        .id(sensor_batch_merge_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(25).unit_code(UnitCode::Cel))
        .build()?;
    broker.entities().create(&sensor_batch_merge).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(sensor_batch_merge_id.clone(), sensor_type.clone())
        .property("temperature", 100)
        .build()?];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_batch_merge_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(100).unit_code(UnitCode::Cel)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_batch_merge_id)
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
                contains(any_attribute().value(100).unit_code(UnitCode::Cel)),
                contains(any_attribute().value(100)).times(eq(1))
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
    expect_that!(page.items(), contains(has_id(&sensor_batch_merge_id)));
    Ok(())
}
