//! An empty array round trips through every reading.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: A Property whose value is the empty array reads back as `[]` from every temporal reading and the current
/// representation
/// Clause: 3.1, 5.2.5, 4.5.7, 4.5.9, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9006_an_empty_array_round_trips_through_every_reading() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_probe_id = namespace.urn("ArrayProbe", 1)?;
    let array_probe_type = namespace.entity_type("ArrayProbe")?;
    let array_probe_evolution = TemporalEntity::builder()
        .id(array_probe_id.clone())
        .entity_type(array_probe_type.clone())
        .property("tags", Property::builder(Value::Array(vec![])).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&array_probe_evolution).send().await?.created()?;
    // Retrieve Temporal Evolution of an Entity.
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("tags", nth(0, property().length_at([], 0))));
    // Query Temporal Evolution of Entities.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&array_probe_type)
        .ids(vec![array_probe_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_evolution("tags", nth(0, any_attribute().length_at([], 0)))));
    // Clause 4.5.9 renders the instance as one [value, instant] pair.
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&array_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let values = retrieved_2.body();
    expect_that!(
        values,
        has_values("tags", nth(0, pair().value(Value::Array(Vec::new())).time("2024-01-01T00:00:00Z".parse()?)))
    );
    // The current representation answers the same value.
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_probe_id)
        .send()
        .await?;
    let entity = retrieved_3.body();
    expect_that!(entity, has_attribute("tags", any_attribute().length_at([], 0)));
    Ok(())
}
