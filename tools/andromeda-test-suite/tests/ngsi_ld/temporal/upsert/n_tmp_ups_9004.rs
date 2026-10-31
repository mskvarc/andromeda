//! An instance with an open ring refuses the whole evolution.

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

/// Pins: Create or Update Temporal Evolution of an Entity whose second instance holds a Polygon with an open ring
/// answers 400 `BadRequestData` and stores neither instance
/// Clause: 5.6.11.4, 4.7.2, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9004_an_instance_with_an_open_ring_refuses_the_whole_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ring_probe_temporal_id = namespace.urn("RingProbe", "temporal")?;
    let ring_probe_type = namespace.entity_type("RingProbe")?;
    let ring_probe_temporal_evolution = TemporalEntity::builder()
        .id(ring_probe_temporal_id.clone())
        .entity_type(ring_probe_type)
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"[{"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}, "observedAt": "2025-01-01T00:00:00Z"}, {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}, "observedAt": "2025-01-01T00:01:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(ring_probe_temporal_evolution.render(Lossless::Normalized))?.set_member("location", &location);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&ring_probe_temporal_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&ring_probe_temporal_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
