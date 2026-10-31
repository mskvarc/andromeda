//! A batch deleted entity leaves its type but keeps its temporal evolution.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType, Value},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: after Batch Entity Delete of two of four Entities of one type, the type's `entityCount` and Query Entities
/// by the type count the two left, while Query Temporal Evolution of Entities by the type over `modifiedAt` still
/// answers all four
/// Clause: 5.6.6.4, 5.6.10.4, 5.6.16.4, 5.7.2.4, 5.7.4.4, 5.7.7.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so each Entity created through the Core API has a
/// Temporal Evolution, which only clause 5.6.16 deletes
#[gtest]
#[tokio::test]
async fn o_bat_del_9004_a_batch_deleted_entity_leaves_its_type_but_keeps_its_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let ids = [
        namespace.urn("Vehicle", "deleted-1")?,
        namespace.urn("Vehicle", "deleted-2")?,
        namespace.urn("Vehicle", "kept-1")?,
        namespace.urn("Vehicle", "kept-2")?,
    ];
    for id in &ids {
        let vehicle = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Value::Number("20.0".parse()?))
            .build()?;
        broker.entities().create(&vehicle).send().await?.done()?;
    }
    let [deleted_1, deleted_2, kept_1, kept_2] = &ids;
    broker.entities().batch_delete(&[deleted_1.clone(), deleted_2.clone()]).send().await?.done()?;
    let information = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_types()
        .retrieve(&vehicle_type)
        .send()
        .await?;
    expect_that!(information.entity_count, eq(2));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(2)));
    expect_that!(page.items(), unordered_elements_are![has_id(kept_1), has_id(kept_2)]);
    let evolutions = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&vehicle_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(evolutions.total(), some(eq(4)));
    expect_that!(
        evolutions.items(),
        unordered_elements_are![has_id(deleted_1), has_id(deleted_2), has_id(kept_1), has_id(kept_2)]
    );
    Ok(())
}
