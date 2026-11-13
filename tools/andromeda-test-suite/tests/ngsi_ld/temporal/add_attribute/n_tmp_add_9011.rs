//! Adding observation space instances as properties is refused.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AddTemporalAttributes},
    model::{ErrorType, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution with `observationSpace` instances written as Properties answers 400
/// `BadRequestData` naming `observationSpace` and adds no instance
/// Clause: 5.2.20, 5.2.4, 5.5.4, 5.6.12.4
#[gtest]
#[tokio::test]
async fn n_tmp_add_9011_adding_observation_space_instances_as_properties_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("GeoMemberProbe", "temporal")?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    let probe = TemporalEntity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type.clone())
        .property("speed", Property::builder(120).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&probe).send().await?.created()?;
    let placeholder = TemporalEntity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type)
        .property("speed", Property::builder(80).observed_at("2025-01-01T00:05:00Z".parse()?))
        .build()?
        .attributes;
    // The model admits `observationSpace` only as a GeoProperty, so its instance is written as text.
    let observation_space: RawJson = r#"[{"type": "Property", "value": "somewhere", "observedAt": "2025-01-01T00:05:00Z"}]"#.parse()?;
    let request = broker
        .unchecked(&AddTemporalAttributes::builder().id(&probe_id).attributes(&placeholder).build())?
        .document(&UncheckedDocument::empty().set_member("observationSpace", &observation_space));
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("'observationSpace' must be a GeoProperty")
    );
    let evolution = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&probe_id)
        .send()
        .await?;
    expect_that!(evolution.body(), has_evolution("speed", len(eq(1))));
    expect_that!(evolution.body(), lacks_evolution("observationSpace"));
    Ok(())
}
