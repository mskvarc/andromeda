//! The conformant version is answered 200 unamended and echoed.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, temporal_prefer_vehicle, versioned_answer::send_versioned};
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

/// Pins: Retrieve Temporal Evolution of an Entity with Prefer ngsi-ld=1.9 answers 200 with Preference-Applied
/// ngsi-ld=1.9 and the body the request without the header answers
/// Clause: 6.3.6, 4.3.6.8, Table 6.19.3.1-2
/// Recorded: Table 6.19.3.1-2 gives 203 to an altered response body and clause 6.3.6 requires the echo whenever the
/// preference is set; a body needing no amendment is not altered, and for the version it conforms to Andromeda answers
/// 200 with the echo
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9024_the_conformant_version_is_answered_200_unamended_and_echoed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_conformant_evolution = temporal_prefer_vehicle::vehicle(&namespace, "conformant")?;
    let vehicle_conformant_id = vehicle_conformant_evolution.id.clone();
    broker.temporal().create_or_update(&vehicle_conformant_evolution).send().await?.created()?;
    let window = TemporalQuery::after("2000-01-01T00:00:00Z".parse()?);
    let operation = RetrieveTemporalEvolution::builder()
        .id(&vehicle_conformant_id)
        .selection(TemporalSelection::Query(&window))
        .format(NormalizedTemporal)
        .build();
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let unadapted = send_versioned(&client, &operation, None).await?;
    expect_that!(unadapted.status, eq(StatusCode::OK));
    let conformant = send_versioned(&client, &operation, Some("1.9")).await?;
    expect_that!(conformant.status, eq(StatusCode::OK));
    expect_that!(conformant.preference_applied.as_deref(), some(eq("ngsi-ld=1.9")));
    expect_that!(&conformant.body, eq(&unadapted.body));
    Ok(())
}
