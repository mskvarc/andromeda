//! A type change is answered under the new type at once.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    type_matcher::lists_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: on a broker that defers its history, Query Temporal Evolution of type B sent right after a second Create or
/// Update Temporal Evolution gave an Entity of type A the type B answers the Entity under both types
/// Clause: 5.6.11.4, 5.7.4.4, 4.5.6
/// Recorded: the deferred temporal append mode is Andromeda's; clause 5.6.11.4 adds the Entity Type names an upsert
/// carries to the Temporal Evolution, clause 4.5.6 makes that evolution the Entity as clause 4.5.1 mandates, and
/// clause 5.7.4.4 selects by those types whether or not the queued history has been applied
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9060_a_type_change_is_answered_under_the_new_type_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_deferred = broker::client(BrokerRole::Deferred)?;
    let shape_id = namespace.urn("Shape", 1)?;
    let type_a = namespace.entity_type("A")?;
    let type_b = namespace.entity_type("B")?;
    let shape = TemporalEntity::builder()
        .id(shape_id.clone())
        .entity_type(type_a.clone())
        .property("level", Property::builder(1).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?;
    broker_deferred.temporal().create_or_update(&shape).send().await?.created()?;
    let shape_retyped = TemporalEntity::builder()
        .id(shape_id.clone())
        .entity_type(type_b.clone())
        .property("level", Property::builder(2).observed_at("2026-03-01T11:00:00Z".parse()?))
        .build()?;
    broker_deferred.temporal().create_or_update(&shape_retyped).send().await?.updated()?;
    let page = broker_deferred
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&type_b)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(has_id(&shape_id), lists_type(&type_a), lists_type(&type_b), has_evolution("level", len(eq(2))))
        )
    );
    Ok(())
}
