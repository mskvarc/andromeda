//! Add temporal attributes records the empty array.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    core_context::core_context,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Add Temporal Attributes of an instance whose value is the empty array answers 204, and both the normalized
/// and the `temporalValues` evolution answer that empty array at its `observedAt`
/// Clause: 5.6.12.4, 4.5.7, 4.5.9, 3.1
#[gtest]
#[tokio::test]
async fn n_tmp_add_9001_add_temporal_attributes_records_the_empty_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let empty_probe_id = namespace.urn("EmptyProbe", 1)?;
    let empty_probe_type = namespace.entity_type("EmptyProbe")?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let empty_probe_evolution = TemporalEntity::builder()
        .id(empty_probe_id.clone())
        .entity_type(empty_probe_type.clone())
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&empty_probe_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(empty_probe_id.clone())
        .entity_type(empty_probe_type.clone())
        .property("tags", Property::builder(Value::Array(vec![])).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?
        .attributes;
    core_context_body.temporal().add_attributes(&empty_probe_id, &instances).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&empty_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("tags", all!(len(eq(1)), nth(0, property().length_at([], 0)))));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&empty_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let values = retrieved_2.body();
    expect_that!(values, has_values("tags", len(eq(1))));
    expect_that!(
        values,
        has_values("tags", nth(0, pair().value(Value::Array(Vec::new())).time("2024-01-02T00:00:00Z".parse()?)))
    );
    Ok(())
}
