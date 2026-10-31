//! A version preference keeps the aggregates.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals},
    broker,
    namespace::Namespace,
    outcome::Upserted,
    temporal_prefer_vehicle,
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{Aggregated, RepresentationSpelling, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity with options=aggregatedValues and totalCount per day over the three
/// days of its observations answers the `LanguageProperty` as such, and with Prefer ngsi-ld=1.0 answers 203 with
/// Preference-Applied ngsi-ld=1.0, the `LanguageProperty` as a Property and the same three daily counts of speed without
/// datasetId
/// Clause: 4.3.6.8, 4.5.19, 6.3.6
/// Recorded: the aggregation members of clause 4.5.19 appear in no row of Tables 4.3.6.8-1 to 4.3.6.8-3; Andromeda
/// reformats the family and removes datasetId, and passes the aggregates through
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9027_a_version_preference_keeps_the_aggregates() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_aggregated_evolution = temporal_prefer_vehicle::vehicle(&namespace, "aggregated")?;
    let vehicle_aggregated_id = vehicle_aggregated_evolution.id.clone();
    broker.temporal().create_or_update(&vehicle_aggregated_evolution).send().await?.created()?;
    let window = temporal_query::between("2026-01-01T00:00:00Z".parse()?, "2026-01-04T00:00:00Z".parse()?)
        .aggregation_methods([AggregationMethod::TotalCount])
        .aggregation_period("P1D".parse()?)
        .build()?;
    let operation = RetrieveTemporalEvolution::builder()
        .id(&vehicle_aggregated_id)
        .selection(TemporalSelection::Query(&window))
        .spelling(RepresentationSpelling::default().in_options())
        .format(Aggregated)
        .build();
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let current = send_versioned(&client, &operation, None).await?;
    expect_that!(current.status, eq(StatusCode::OK));
    // The version 1.0 amendment writes shapes the model reads as current ones, so both answers are read as received.
    let body = &current.body;
    expect_that!(body, json_equals("$.label.type", "LanguageProperty"));
    expect_that!(body, json_count("$.speed.totalCount", 3));
    for index in 0..3 {
        expect_that!(body, json_equals(&format!("$.speed.totalCount[{index}][0]"), 1));
    }
    let adapted = send_versioned(&client, &operation, Some("1.0")).await?;
    expect_that!(adapted.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(adapted.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    let body = &adapted.body;
    expect_that!(body, json_equals("$.label.type", "Property"));
    expect_that!(body, json_absent("$.speed.datasetId"));
    expect_that!(body, json_count("$.speed.totalCount", 3));
    for index in 0..3 {
        expect_that!(body, json_equals(&format!("$.speed.totalCount[{index}][0]"), 1));
    }
    Ok(())
}
