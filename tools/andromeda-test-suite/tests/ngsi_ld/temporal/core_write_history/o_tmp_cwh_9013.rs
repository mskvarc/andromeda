//! A batch update without overwrite records nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Batch Entity Update with `options=noOverwrite` naming the held temperature with 99 answers 207; the Entity
/// keeps temperature 25, the Temporal Evolution keeps its one instance, and q does not select temperature==99
/// Clause: 5.6.9.4, 5.6.9.5, 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9013_a_batch_update_without_overwrite_records_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_batch_no_overwrite_id = namespace.urn("Sensor", "batch-no-overwrite")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_batch_no_overwrite = Entity::builder()
        .id(sensor_batch_no_overwrite_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", 25)
        .build()?;
    broker.entities().create(&sensor_batch_no_overwrite).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(sensor_batch_no_overwrite_id.clone(), sensor_type.clone())
        .property("temperature", 99)
        .build()?];
    broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.partial()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_batch_no_overwrite_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(25)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_batch_no_overwrite_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["temperature".parse()?])
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution, has_evolution("temperature", all!(len(eq(1)), nth(0, any_attribute().value(25)))));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(99))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
