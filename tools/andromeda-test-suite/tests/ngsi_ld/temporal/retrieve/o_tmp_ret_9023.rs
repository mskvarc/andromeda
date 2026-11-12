//! A version amendment keeps every instance in its place.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
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
        operation::{NormalizedTemporal, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::NgsiLdMediaType,
    query::temporal_query::TemporalQuery,
};

/// Pins: Retrieve Temporal Evolution of an Entity answers 200 with no Preference-Applied and the speed instances
/// observed on 2026-01-01, 01-02 and 01-03, and with Prefer ngsi-ld=1.0 answers the same three instants in the same
/// order
/// Clause: 4.3.6.8, 4.5.7, 6.3.6
/// Recorded: Tables 4.3.6.8-1 to 4.3.6.8-3 give the fallbacks of Entity data, where a 1.0 target keeps one instance of
/// an Attribute (notes 2 and 3) and loses its observedAt; the Attribute of a Temporal Evolution is the sequence of its
/// instances, each placed in it by its Temporal Property (clause 4.5.7), and Andromeda amends each instance, keeping
/// every instance and its observedAt
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9023_a_version_amendment_keeps_every_instance_in_its_place() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_sequence_evolution = temporal_prefer_vehicle::vehicle(&namespace, "sequence")?;
    let vehicle_sequence_id = vehicle_sequence_evolution.id.clone();
    broker.temporal().create_or_update(&vehicle_sequence_evolution).send().await?.created()?;
    let window = TemporalQuery::after("2000-01-01T00:00:00Z".parse()?);
    let operation = RetrieveTemporalEvolution::builder()
        .id(&vehicle_sequence_id)
        .selection(TemporalSelection::Query(&window))
        .format(NormalizedTemporal)
        .build();
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let unadapted = send_versioned(&client, &operation, None).await?;
    expect_that!(unadapted.status, eq(StatusCode::OK));
    expect_that!(unadapted.preference_applied, none());
    let adapted = send_versioned(&client, &operation, Some("1.0")).await?;
    expect_that!(adapted.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    // The version 1.0 amendment writes shapes the model reads as current ones, so both answers are read as received.
    for answer in [&unadapted, &adapted] {
        let body = &answer.body;
        expect_that!(body, json_count("$.speed", 3));
        expect_that!(body, json_equals("$.speed[0].observedAt", "2026-01-01T00:00:00Z"));
        expect_that!(body, json_equals("$.speed[1].observedAt", "2026-01-02T00:00:00Z"));
        expect_that!(body, json_equals("$.speed[2].observedAt", "2026-01-03T00:00:00Z"));
    }
    Ok(())
}
