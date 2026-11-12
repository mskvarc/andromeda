//! A preference naming no version is not echoed.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    prefer_text::{preference_applied, send_preferring},
    temporal_prefer_vehicle,
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

/// Pins: Retrieve Temporal Evolution of an Entity with Prefer ngsi-ld=latest answers 200 without Preference-Applied
/// Clause: 6.3.6, 4.3.6.8
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9025_a_preference_naming_no_version_is_not_echoed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unreadable_evolution = temporal_prefer_vehicle::vehicle(&namespace, "unreadable")?;
    let vehicle_unreadable_id = vehicle_unreadable_evolution.id.clone();
    broker.temporal().create_or_update(&vehicle_unreadable_evolution).send().await?.created()?;
    let window = TemporalQuery::after("2000-01-01T00:00:00Z".parse()?);
    let operation = RetrieveTemporalEvolution::builder()
        .id(&vehicle_unreadable_id)
        .selection(TemporalSelection::Query(&window))
        .format(NormalizedTemporal)
        .build();
    // The typed preference names versions only, so the text `latest` is sent as written.
    let answer = send_preferring(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &operation, "ngsi-ld=latest").await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    expect_that!(preference_applied(answer.head.headers()), none());
    Ok(())
}
