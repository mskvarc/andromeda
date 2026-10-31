//! A position off the wgs 84 axes is refused and not created.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with a location Point at longitude 1000, at latitude 90.000001, or an `observationSpace`
/// Polygon with a vertex at latitude 95 answers 400 `BadRequestData` naming the refused value, and stores nothing
/// Clause: 4.7, 4.7.2, 5.5.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9012_a_position_off_the_wgs_84_axes_is_refused_and_not_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let range_probe_both_axes_id = namespace.urn("RangeProbe", "both-axes")?;
    let range_probe_type = namespace.entity_type("RangeProbe")?;
    let range_probe_past_pole_id = namespace.urn("RangeProbe", "past-pole")?;
    let range_probe_observed_vertex_id = namespace.urn("RangeProbe", "observed-vertex")?;
    let range_probe_both_axes = Entity::builder()
        .id(range_probe_both_axes_id.clone())
        .entity_type(range_probe_type.clone())
        .build()?;
    // The model refuses it (a position the constructor refuses: longitude 1000 lies outside [-180, 180]), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1000, 2000]}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&range_probe_both_axes, Lossless::Normalized).set_member("location", &location);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&range_probe_both_axes).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("1000")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_both_axes_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let range_probe_past_pole = Entity::builder()
        .id(range_probe_past_pole_id.clone())
        .entity_type(range_probe_type.clone())
        .build()?;
    // The model refuses it (a position the constructor refuses: latitude 90.000001 lies outside [-90, 90]), so `location` is written as text.
    let location_2: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, 90.000001]}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&range_probe_past_pole, Lossless::Normalized).set_member("location", &location_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&range_probe_past_pole).build())?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("90.000001")
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_past_pole_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let range_probe_observed_vertex = Entity::builder()
        .id(range_probe_observed_vertex_id.clone())
        .entity_type(range_probe_type.clone())
        .build()?;
    // The model refuses it (a position the constructor refuses: latitude 95 lies outside [-90, 90]), so `observationSpace` is written as text.
    let observation_space: RawJson =
        r#"{"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [10, 95], [10, 0], [0, 0]]]}}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&range_probe_observed_vertex, Lossless::Normalized).set_member("observationSpace", &observation_space);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&range_probe_observed_vertex).build())?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("observationSpace")
            .with_detail_containing("95")
    );
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_observed_vertex_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    Ok(())
}
