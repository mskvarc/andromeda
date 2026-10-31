//! A version preference amends the temporal query and is echoed.

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
    http::{headers::Accept, operation::QueryTemporalEvolution},
    model::{Lossless, NgsiLdMediaType},
    query::{builders::SelectorTypeBuilder, temporal_query::TemporalQuery},
};

/// Pins: Query Temporal Evolution of Entities with Prefer ngsi-ld=1.0 answers 203 with Preference-Applied ngsi-ld=1.0
/// and the selected Entity amended as the single-entity retrieval amends it
/// Clause: 6.3.6, 4.3.6.8, 4.5.7, Table 6.18.3.2-2
/// Recorded: Tables 4.3.6.8-1 to 4.3.6.8-3 give the fallbacks of Entity data, where a 1.0 target keeps one instance of
/// an Attribute (notes 2 and 3) and loses its observedAt; the Attribute of a Temporal Evolution is the sequence of its
/// instances, each placed in it by its Temporal Property (clause 4.5.7), and Andromeda amends each instance, keeping
/// every instance and its observedAt
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9031_a_version_preference_amends_the_temporal_query_and_is_echoed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_query_evolution = temporal_prefer_vehicle::vehicle(&namespace, "query")?;
    let vehicle_query_id = vehicle_query_evolution.id.clone();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_target_id = namespace.urn("Building", "target")?;
    broker.temporal().create_or_update(&vehicle_query_evolution).send().await?.created()?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let window = TemporalQuery::after("2000-01-01T00:00:00Z".parse()?);
    let ids = [vehicle_query_id];
    let query = QueryTemporalEvolution::builder()
        .entity_type(&selector)
        .ids(&ids)
        .temporal_q(&window)
        .format(Lossless::Normalized)
        .build();
    let answer = send_versioned(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &query, Some("1.0")).await?;
    expect_that!(answer.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(answer.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    // The version 1.0 amendment writes shapes the model reads as current ones, so the answer is read as received.
    let body = &answer.body;
    expect_that!(body, json_count("$", 1));
    expect_that!(body, json_equals("$[0].type", vehicle_type.as_str()));
    expect_that!(body, json_absent("$[0].scope"));
    expect_that!(body, json_count("$[0].speed", 3));
    for (index, (value, observed_at)) in [(1, "2026-01-01T00:00:00Z"), (2, "2026-01-02T00:00:00Z"), (3, "2026-01-03T00:00:00Z")]
        .into_iter()
        .enumerate()
    {
        expect_that!(body, json_equals(&format!("$[0].speed[{index}].value"), value));
        expect_that!(body, json_equals(&format!("$[0].speed[{index}].observedAt"), observed_at));
        expect_that!(body, json_absent(&format!("$[0].speed[{index}].unitCode")));
        expect_that!(body, json_absent(&format!("$[0].speed[{index}].datasetId")));
    }
    expect_that!(body, json_equals("$[0].label[0].type", "Property"));
    expect_that!(body, json_equals("$[0].label[0].value.en", "Pump"));
    expect_that!(body, json_absent("$[0].label[0].languageMap"));
    expect_that!(body, json_equals("$[0].label[0].observedAt", "2026-01-01T00:00:00Z"));
    expect_that!(body, json_equals("$[0].refBuilding[0].object", building_target_id.to_string()));
    expect_that!(body, json_absent("$[0].refBuilding[0].objectType"));
    expect_that!(body, json_equals("$[0].refBuilding[0].observedAt", "2026-01-01T00:00:00Z"));
    Ok(())
}
