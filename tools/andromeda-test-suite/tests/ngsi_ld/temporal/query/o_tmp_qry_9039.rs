//! A modified at window reads every recorded write.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{
        q,
        temporal_query::{self, TemporalProperty},
    },
};

/// Pins: Query Temporal Evolution of Entities with `timeproperty=modifiedAt` over two Batch Entity Upserts writing
/// v 200 then 5 selects the Entity for v>100 and v==5, and on `observedAt` selects nothing for v>100
/// Clause: 5.7.4.4, 4.11, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9039_a_modified_at_window_reads_every_recorded_write() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_modified_id = namespace.urn("TqModified", 1)?;
    let tq_modified_type = namespace.entity_type("TqModified")?;
    let entities = [Entity::builder()
        .id(tq_modified_id.clone())
        .entity_type(tq_modified_type.clone())
        .property("v", 200)
        .build()?];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let entities_2 = [Entity::builder()
        .id(tq_modified_id.clone())
        .entity_type(tq_modified_type.clone())
        .property("v", 5)
        .build()?];
    broker.entities().batch_upsert(&entities_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&tq_modified_type)
        .q(q::attr("v").gt(100))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tq_modified_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&tq_modified_type)
        .q(q::attr("v").eq(5))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&tq_modified_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_modified_type)
        .q(q::attr("v").gt(100))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
