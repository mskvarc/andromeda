//! A temporal evolution of location as a property is refused.

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

/// Pins: Create or Update Temporal Evolution with `location` instances written as Properties answers 400
/// `BadRequestData` naming `location` and stores nothing
/// Clause: 5.2.20, 5.2.4, 5.5.4, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9031_a_temporal_evolution_of_location_as_a_property_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("GeoMemberProbe", "temporal")?;
    let probe_evolution = TemporalEntity::builder()
        .id(probe_id.clone())
        .entity_type(namespace.entity_type("GeoMemberProbe")?)
        .build()?;
    // The model admits `location` only as a GeoProperty, so its instances are written as text.
    let location: RawJson = r#"[{"type": "Property", "value": "here", "observedAt": "2025-01-01T00:00:00Z"}, {"type": "Property", "value": "there", "observedAt": "2025-01-01T00:01:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(probe_evolution.render(Lossless::Normalized))?.set_member("location", &location);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&probe_evolution).build())?
        .document(&document);
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("'location' must be a GeoProperty")
    );
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&probe_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    Ok(())
}
