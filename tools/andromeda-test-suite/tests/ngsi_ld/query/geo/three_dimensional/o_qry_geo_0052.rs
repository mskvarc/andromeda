//! Batch create reports a four ordinate entity in its own error.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::problem,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{
        Entity,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
};

/// Pins: Batch Entity Creation reports an entity whose position has four ordinates as a `BadRequestData` error of
/// its own, still creates the valid entity and stores the refused one nowhere
/// Clause: 4.7.1, 5.6.7, 5.6.7.4
/// Recorded: RFC 7946 section 3.1.1 says implementations "SHOULD NOT extend positions beyond three elements" and
/// that parsers "MAY ignore" extra elements, and 4.7.1 requires a valid `GeoJSON` geometry without saying what a
/// fourth element is, so the specification does not require the refusal; Andromeda refuses a position of four
/// ordinates as `BadRequestData` (Table 5.5.2-1)
#[gtest]
#[tokio::test]
async fn o_qry_geo_0052_batch_create_reports_a_four_ordinate_entity_in_its_own_error() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_good_id = namespace.urn("Building", "good")?;
    let building_type = namespace.entity_type("Building")?;
    let building_bad_id = namespace.urn("Building", "bad")?;
    let building_good = Entity::builder()
        .id(building_good_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?))
        .build()?;
    let document = UncheckedDocument::from_entity(&building_good, Lossless::Normalized);
    let building_bad = Entity::builder().id(building_bad_id.clone()).entity_type(building_type.clone()).build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, 300.0, 7.0]}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_bad, Lossless::Normalized).set_member("location", &location);
    let entities = [building_good, building_bad];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_good_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(0, batch_error().entity(&building_bad_id).problem(ErrorType::BadRequestData))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_good_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("location", any_attribute().length_at(["coordinates"], 3)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_bad_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
