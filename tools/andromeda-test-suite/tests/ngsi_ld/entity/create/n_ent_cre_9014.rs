//! An open or short ring or a one position line is refused and not created.

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

/// Pins: Create Entity with a Polygon whose ring is open, a Polygon whose ring has three positions, a
/// `MultiPolygon` whose second polygon has an open hole, or a `LineString` of one position answers 400
/// `BadRequestData` and creates nothing
/// Clause: 4.7.2, 5.5.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9014_an_open_or_short_ring_or_a_one_position_line_is_refused_and_not_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ring_probe_open_ring_id = namespace.urn("RingProbe", "open-ring")?;
    let ring_probe_type = namespace.entity_type("RingProbe")?;
    let ring_probe_short_ring_id = namespace.urn("RingProbe", "short-ring")?;
    let ring_probe_open_hole_id = namespace.urn("RingProbe", "open-hole")?;
    let ring_probe_short_line_id = namespace.urn("RingProbe", "short-line")?;
    let ring_probe_open_ring = Entity::builder()
        .id(ring_probe_open_ring_id.clone())
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&ring_probe_open_ring, Lossless::Normalized).set_member("location", &location);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&ring_probe_open_ring).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ring_probe_open_ring_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let ring_probe_short_ring = Entity::builder()
        .id(ring_probe_short_ring_id.clone())
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at observationSpace), so `observationSpace` is written as text.
    let observation_space: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [0, 0]]]}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&ring_probe_short_ring, Lossless::Normalized).set_member("observationSpace", &observation_space);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&ring_probe_short_ring).build())?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ring_probe_short_ring_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let ring_probe_open_hole = Entity::builder()
        .id(ring_probe_open_hole_id.clone())
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at operationSpace), so `operationSpace` is written as text.
    let operation_space: RawJson = r#"{"type": "GeoProperty", "value": {"type": "MultiPolygon", "coordinates": [[[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]], [[[10, 10], [20, 10], [20, 20], [10, 20], [10, 10]], [[12, 12], [14, 12], [14, 14], [12, 14]]]]}}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&ring_probe_open_hole, Lossless::Normalized).set_member("operationSpace", &operation_space);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&ring_probe_open_hole).build())?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ring_probe_open_hole_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let ring_probe_short_line = Entity::builder()
        .id(ring_probe_short_line_id.clone())
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location_2: RawJson = r#"{"type": "GeoProperty", "value": {"type": "LineString", "coordinates": [[0, 0]]}}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&ring_probe_short_line, Lossless::Normalized).set_member("location", &location_2);
    let request_4 = broker
        .unchecked(&CreateEntity::builder().entity(&ring_probe_short_line).build())?
        .document(&document_4);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ring_probe_short_line_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    Ok(())
}
