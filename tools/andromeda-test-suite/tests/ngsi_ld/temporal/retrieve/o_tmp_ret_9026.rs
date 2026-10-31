//! A version preference reformats the plural member of temporal values.

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
        operation::{RepresentationSpelling, RetrieveTemporalEvolution, SimplifiedTemporal, TemporalSelection},
    },
    model::NgsiLdMediaType,
    query::temporal_query::TemporalQuery,
};

/// Pins: Retrieve Temporal Evolution of an Entity with options=temporalValues answers the `LanguageProperty` with
/// languageMaps, and with Prefer ngsi-ld=1.0 answers 203 with Preference-Applied ngsi-ld=1.0, the `LanguageProperty` as a
/// Property whose values pair its map with its instant, and speed holding the same values without datasetId
/// Clause: 4.3.6.8, 4.5.9, 6.3.6
/// Recorded: Table 4.3.6.8-1 reformats a `LanguageProperty` as a Property for 1.0 and does not say how the simplified
/// temporal representation of clause 4.5.9 is reformatted; Andromeda renames the plural member and unwraps each pair's
/// map
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9026_a_version_preference_reformats_the_plural_member_of_temporal_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_values_evolution = temporal_prefer_vehicle::vehicle(&namespace, "values")?;
    let vehicle_values_id = vehicle_values_evolution.id.clone();
    broker.temporal().create_or_update(&vehicle_values_evolution).send().await?.created()?;
    let window = TemporalQuery::after("2000-01-01T00:00:00Z".parse()?);
    let operation = RetrieveTemporalEvolution::builder()
        .id(&vehicle_values_id)
        .selection(TemporalSelection::Query(&window))
        .spelling(RepresentationSpelling::default().in_options())
        .format(SimplifiedTemporal)
        .build();
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let current = send_versioned(&client, &operation, None).await?;
    expect_that!(current.status, eq(StatusCode::OK));
    // The version 1.0 amendment writes shapes the model reads as current ones, so both answers are read as received.
    let body = &current.body;
    expect_that!(body, json_equals("$.label.type", "LanguageProperty"));
    expect_that!(body, json_count("$.label.languageMaps", 1));
    expect_that!(body, json_count("$.label.languageMaps[0]", 2));
    expect_that!(body, json_equals("$.label.languageMaps[0][0].languageMap.en", "Pump"));
    expect_that!(body, json_equals("$.label.languageMaps[0][1]", "2026-01-01T00:00:00Z"));
    let adapted = send_versioned(&client, &operation, Some("1.0")).await?;
    expect_that!(adapted.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(adapted.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    let body = &adapted.body;
    expect_that!(body, json_equals("$.label.type", "Property"));
    expect_that!(body, json_absent("$.label.languageMaps"));
    expect_that!(body, json_count("$.label.values", 1));
    expect_that!(body, json_count("$.label.values[0]", 2));
    expect_that!(body, json_equals("$.label.values[0][0].en", "Pump"));
    expect_that!(body, json_equals("$.label.values[0][1]", "2026-01-01T00:00:00Z"));
    expect_that!(body, json_equals("$.speed.type", "Property"));
    expect_that!(body, json_absent("$.speed.datasetId"));
    expect_that!(body, json_count("$.speed.values", 3));
    for (index, (value, observed_at)) in [(1, "2026-01-01T00:00:00Z"), (2, "2026-01-02T00:00:00Z"), (3, "2026-01-03T00:00:00Z")]
        .into_iter()
        .enumerate()
    {
        expect_that!(body, json_count(&format!("$.speed.values[{index}]"), 2));
        expect_that!(body, json_equals(&format!("$.speed.values[{index}][0]"), value));
        expect_that!(body, json_equals(&format!("$.speed.values[{index}][1]"), observed_at));
    }
    Ok(())
}
