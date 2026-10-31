//! A relationship aggregate is labelled relationship.

use andromeda_test_suite::{
    aggregation_matcher::{aggregated_as, has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Relationship,
        evolution::{AggregatedKind, AggregationMethod, TemporalEntity},
    },
};

/// Pins: the aggregated representation of a Relationship is labelled with type Relationship
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0032_a_relationship_aggregate_is_labelled_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let parking_p1_id = namespace.urn("Parking", "P1")?;
    let parking_p2_id = namespace.urn("Parking", "P2")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .relationship(
            "isParked",
            Relationship::builder(parking_p1_id.clone()).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .relationship("isParked", Relationship::builder(parking_p2_id).observed_at("2024-01-01T01:00:00Z".parse()?))
        .relationship(
            "isParked",
            Relationship::builder(parking_p1_id.clone()).observed_at("2024-01-01T02:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("totalCount".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated, aggregated_as("isParked", AggregatedKind::Relationship));
    expect_that!(aggregated, has_aggregate("isParked", AggregationMethod::TotalCount, nth(0, period().value(3))));
    Ok(())
}
