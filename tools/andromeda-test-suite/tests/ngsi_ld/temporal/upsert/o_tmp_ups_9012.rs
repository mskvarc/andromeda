//! A newer temporal append advances current state and a backfill does not.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{
        q,
        temporal_query::{self, TemporalProperty},
    },
};

/// Pins: Appending to the Temporal Evolution a v of 200 observed in 2030 to an Entity whose v is 5 observed in 2021
/// makes Retrieve Entity and q answer 200, and a later backfill of 1 observed in 2019 leaves them at 200 while the
/// Temporal Evolution holds 1, 5 and 200
/// Clause: 4.5.5.3, 5.6.11.4, 5.6.12.4
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9012_a_newer_temporal_append_advances_current_state_and_a_backfill_does_not() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let temporal_append_id = namespace.urn("TemporalAppend", 1)?;
    let temporal_append_type = namespace.entity_type("TemporalAppend")?;
    let temporal_append = Entity::builder()
        .id(temporal_append_id.clone())
        .entity_type(temporal_append_type.clone())
        .property("v", Property::builder(5).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&temporal_append).send().await?.done()?;
    let instances = TemporalEntity::builder()
        .id(temporal_append_id.clone())
        .entity_type(temporal_append_type.clone())
        .property("v", Property::builder(200).observed_at("2030-01-01T00:00:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&temporal_append_id, &instances).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&temporal_append_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("v", any_attribute().value(200)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&temporal_append_type)
        .q(q::attr("v").eq(200))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&temporal_append_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&temporal_append_type)
        .q(q::attr("v").eq(5))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let temporal_append_evolution = TemporalEntity::builder()
        .id(temporal_append_id.clone())
        .entity_type(temporal_append_type.clone())
        .property("v", Property::builder(1).observed_at("2019-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&temporal_append_evolution).send().await?.updated()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&temporal_append_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("v", any_attribute().value(200)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&temporal_append_type)
        .q(q::attr("v").eq(1))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&temporal_append_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .attrs(vec!["v".parse()?])
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(
        evolution,
        has_evolution(
            "v",
            all!(
                len(eq(3)),
                contains(any_attribute().value(1)),
                contains(any_attribute().value(5)),
                contains(any_attribute().value(200))
            )
        )
    );
    Ok(())
}
