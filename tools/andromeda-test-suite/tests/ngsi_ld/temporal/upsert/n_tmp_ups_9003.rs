//! A temporal evolution off the axes is refused.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution with two location instances, the second at longitude 181, answers 400
/// `BadRequestData` naming the refused value, and stores nothing
/// Clause: 5.6.11.4, 4.7
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9003_a_temporal_evolution_off_the_axes_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let range_probe_temporal_id = namespace.urn("RangeProbe", "temporal")?;
    let range_probe_type = namespace.entity_type("RangeProbe")?;
    let range_probe_temporal_evolution = TemporalEntity::builder()
        .id(range_probe_temporal_id.clone())
        .entity_type(range_probe_type)
        .build()?;
    // The model refuses it (a position the constructor refuses: longitude 181 lies outside [-180, 180]), so `location` is written as text.
    let location: RawJson = r#"[{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [14.5, 46.05]}, "observedAt": "2025-01-01T00:00:00Z"}, {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [181, 0]}, "observedAt": "2025-01-01T00:01:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(range_probe_temporal_evolution.render(Lossless::Normalized))?.set_member("location", &location);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&range_probe_temporal_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("181")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&range_probe_temporal_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
