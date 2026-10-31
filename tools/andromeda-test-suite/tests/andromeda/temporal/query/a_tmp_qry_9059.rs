//! A written entity is answered by a temporal query at once.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: on a broker that defers its history, Query Temporal Evolution sent right after Create or Update Temporal
/// Evolution answers the Entity with its one speed instance, whether or not the queued history has been applied
/// Clause: 5.6.11, 5.7.4
/// Recorded: the deferred temporal append mode is Andromeda's; clause 5.7.4 selects the Entities whose Attribute
/// instances fall in the window, and the instance a write recorded is one of them from the moment the write is
/// acknowledged
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9059_a_written_entity_is_answered_by_a_temporal_query_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car = TemporalEntity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(42).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&car).send().await?.created()?;
    let page = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&car_id),
                has_evolution(
                    "speed",
                    all!(len(eq(1)), nth(0, property().value(42).observed_at("2026-03-01T10:00:00Z".parse()?)))
                )
            )
        )
    );
    Ok(())
}
