//! Each dataset is aggregated on its own.

use andromeda_test_suite::{
    aggregated_answer::{retrieve_unread, unbounded},
    body_json::json_count,
    broker,
    json_scalar::json_number_equals,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::AggregationMethods,
};

/// Pins: avg over a Property with default instances 100 and 200 and radar instances 300 and 400 answers two
/// aggregates, 150 for the default instances and 350 for the radar dataset
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: 4.5.19 describes one aggregate per Attribute and does not say how `datasetIds` are kept apart;
/// Andromeda aggregates each dataset on its own, as 4.5.5 keeps them apart
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0043_each_dataset_is_aggregated_on_its_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let radar = namespace.dataset_id("Dataset", "radar")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(200).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property(
            "speed",
            Property::builder(300).observed_at("2024-01-01T00:00:00Z".parse()?).dataset_id(radar.clone()),
        )
        .property(
            "speed",
            Property::builder(400).observed_at("2024-01-01T01:00:00Z".parse()?).dataset_id(radar.clone()),
        )
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    // The answer holds the attribute once per dataset as an array, which the typed aggregated reader cannot hold, so
    // the typed request is answered as received.
    let methods = "avg".parse::<AggregationMethods>()?;
    let aggregated = retrieve_unread(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &agg_id, unbounded(&methods), &[]).await?;
    expect_that!(aggregated.status(), eq(StatusCode::OK));
    expect_that!(aggregated.body(), json_count("$.speed", 2));
    expect_that!(
        aggregated.body(),
        json_number_equals(&format!("$.speed[?(@.datasetId == '{radar}')].avg[0][0]"), 350.0)
    );
    expect_that!(aggregated.body(), json_number_equals("$.speed[?(!@.datasetId)].avg[0][0]", 150.0));
    Ok(())
}
