//! A backfill under a new type is answered under both types at once.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eventually::retried,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    type_matcher::{lists_type, type_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: on a broker that defers its history, a second Create or Update Temporal Evolution naming B for an Entity of
/// type A, its one instance observed before the recorded one, is answered at once by a temporal query on A and on B,
/// each listing the Entity under both types whether or not the queued history has been applied, and the Temporal
/// Evolution read back holds both instances and both types
/// Clause: 5.6.11.4, 5.7.4.4, 4.5.6
/// Recorded: the deferred temporal append mode is Andromeda's; clause 5.6.11.4 adds the Entity Type names an upsert
/// carries to the Temporal Evolution's list and removes none, and clause 5.7.4.4 selects by that list whether or not
/// the queued history has been applied
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9067_a_backfill_under_a_new_type_is_answered_under_both_types_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_deferred = broker::client(BrokerRole::Deferred)?;
    let shape_id = namespace.urn("Shape", 1)?;
    let type_a = namespace.entity_type("A")?;
    let type_b = namespace.entity_type("B")?;
    let shape = TemporalEntity::builder()
        .id(shape_id.clone())
        .entity_type(type_a.clone())
        .property("level", Property::builder(1).observed_at("2026-03-01T11:00:00Z".parse()?))
        .build()?;
    broker_deferred.temporal().create_or_update(&shape).send().await?.created()?;
    let backfill = TemporalEntity::builder()
        .id(shape_id.clone())
        .entity_type(type_b.clone())
        .property("level", Property::builder(2).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?;
    broker_deferred.temporal().create_or_update(&backfill).send().await?.updated()?;
    for queried in [&type_a, &type_b] {
        let page = broker_deferred
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
            .entity_type(queried)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)));
        expect_that!(page.items(), nth(0, all!(lists_type(&type_a), lists_type(&type_b))));
    }
    retried(async || {
        let retrieved = broker_deferred
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve(&shape_id)
            .send()
            .await?;
        let evolution = retrieved.body();
        verify_that!(evolution, all!(has_evolution("level", len(eq(2))), type_count(2)))?;
        Ok(())
    })
    .await?;
    let page_2 = broker_deferred
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&type_a)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, all!(has_id(&shape_id), lists_type(&type_b))));
    Ok(())
}
