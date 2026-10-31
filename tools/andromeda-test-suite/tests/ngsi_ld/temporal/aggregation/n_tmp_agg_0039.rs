//! The representation holds id type and triples.

use andromeda_test_suite::{
    aggregation_matcher::{aggregated_as, has_aggregate},
    broker,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        OneOrMany,
        Property,
        evolution::{AggregatedKind, AggregationMethod, TemporalEntity},
    },
};

/// Pins: the aggregated representation holds the Entity id and type, the Property labelled Property, and each
/// method as an array of triples of exactly three elements
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0039_the_representation_holds_id_type_and_triples() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type.clone())
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(200).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(300).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    // The reader accepts a period only as a triple of value, start and end, so a read that succeeds proves each is one.
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("avg".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated.id, eq(&agg_id));
    expect_that!(aggregated.entity_type, eq(&OneOrMany::one(agg_type)));
    expect_that!(aggregated, aggregated_as("speed", AggregatedKind::Property));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Avg, not(is_empty())));
    Ok(())
}
