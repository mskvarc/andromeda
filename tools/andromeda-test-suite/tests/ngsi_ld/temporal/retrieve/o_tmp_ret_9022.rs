//! A version preference amends the temporal evolution and is echoed.

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
        operation::{RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{Lossless, NgsiLdMediaType},
    query::temporal_query::TemporalQuery,
};

/// Pins: Retrieve Temporal Evolution of an Entity with Prefer ngsi-ld=1.0 answers 203 with Preference-Applied
/// ngsi-ld=1.0, the first type alone, no scope, each speed instance without unitCode and datasetId, the `LanguageProperty`
/// as a Property holding its map and the Relationship without objectType, every instance keeping its observedAt
/// Clause: 6.3.6, 4.3.6.8, 4.5.7, Table 6.19.3.1-2
/// Recorded: Tables 4.3.6.8-1 to 4.3.6.8-3 give the fallbacks of Entity data, where a 1.0 target keeps one instance of
/// an Attribute (notes 2 and 3) and loses its observedAt; the Attribute of a Temporal Evolution is the sequence of its
/// instances, each placed in it by its Temporal Property (clause 4.5.7), and Andromeda amends each instance, keeping
/// every instance and its observedAt
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9022_a_version_preference_amends_the_temporal_evolution_and_is_echoed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_retrieve_evolution = temporal_prefer_vehicle::vehicle(&namespace, "retrieve")?;
    let vehicle_retrieve_id = vehicle_retrieve_evolution.id.clone();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_target_id = namespace.urn("Building", "target")?;
    broker.temporal().create_or_update(&vehicle_retrieve_evolution).send().await?.created()?;
    let window = TemporalQuery::after("2000-01-01T00:00:00Z".parse()?);
    let operation = RetrieveTemporalEvolution::builder()
        .id(&vehicle_retrieve_id)
        .selection(TemporalSelection::Query(&window))
        .format(Lossless::Normalized)
        .build();
    let answer = send_versioned(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &operation, Some("1.0")).await?;
    expect_that!(answer.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(answer.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    // The version 1.0 amendment writes shapes the model reads as current ones, so the answer is read as received.
    let body = &answer.body;
    expect_that!(body, json_equals("$.type", vehicle_type.as_str()));
    expect_that!(body, json_absent("$.scope"));
    expect_that!(body, json_count("$.speed", 3));
    for (index, (value, observed_at)) in [(1, "2026-01-01T00:00:00Z"), (2, "2026-01-02T00:00:00Z"), (3, "2026-01-03T00:00:00Z")]
        .into_iter()
        .enumerate()
    {
        expect_that!(body, json_equals(&format!("$.speed[{index}].value"), value));
        expect_that!(body, json_equals(&format!("$.speed[{index}].observedAt"), observed_at));
        expect_that!(body, json_absent(&format!("$.speed[{index}].unitCode")));
        expect_that!(body, json_absent(&format!("$.speed[{index}].datasetId")));
    }
    expect_that!(body, json_equals("$.label[0].type", "Property"));
    expect_that!(body, json_equals("$.label[0].value.en", "Pump"));
    expect_that!(body, json_absent("$.label[0].languageMap"));
    expect_that!(body, json_equals("$.label[0].observedAt", "2026-01-01T00:00:00Z"));
    expect_that!(body, json_equals("$.refBuilding[0].object", building_target_id.to_string()));
    expect_that!(body, json_absent("$.refBuilding[0].objectType"));
    expect_that!(body, json_equals("$.refBuilding[0].observedAt", "2026-01-01T00:00:00Z"));
    Ok(())
}
