//! An observed at keeps its milliseconds.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity whose instances were observed at 10:00:00.123Z and 10:00:01.456Z
/// answers both `observedAt` as written
/// Clause: 4.6.3, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9014_an_observed_at_keeps_its_milliseconds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let t_milliseconds_id = namespace.urn("T", "milliseconds")?;
    let t_type = namespace.entity_type("T")?;
    let t_milliseconds_evolution = TemporalEntity::builder()
        .id(t_milliseconds_id.clone())
        .entity_type(t_type)
        .property("p", Property::builder(1).observed_at("2026-03-01T10:00:00.123Z".parse()?))
        .property("p", Property::builder(2).observed_at("2026-03-01T10:00:01.456Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&t_milliseconds_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&t_milliseconds_id)
        .temporal_q(temporal_query::after("2026-03-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "p",
            all!(
                len(eq(2)),
                contains(any_attribute().value(1).observed_at("2026-03-01T10:00:00.123Z".parse()?)),
                contains(any_attribute().value(1)).times(eq(1)),
                contains(any_attribute().value(2).observed_at("2026-03-01T10:00:01.456Z".parse()?)),
                contains(any_attribute().value(2)).times(eq(1))
            )
        )
    );
    Ok(())
}
