//! A batch refuses only the entity off the axes.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        UncheckedDocument,
        headers::Accept,
        operation::{BatchCreate, BatchUpsert},
    },
    model::{
        Entity,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
};

/// Pins: Batch Entity Creation and Batch Entity Upsert of an Entity at a valid position and one at latitude 91 each
/// answer 207 with the first a success and the second an error of type `BadRequestData` naming the refused value,
/// and only the first is stored
/// Clause: 5.6.7.4, 5.6.8.4, 4.7
#[gtest]
#[tokio::test]
async fn n_bat_res_9010_a_batch_refuses_only_the_entity_off_the_axes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let range_probe_batch_create_valid_id = namespace.urn("RangeProbe", "batch-create-valid")?;
    let range_probe_type = namespace.entity_type("RangeProbe")?;
    let range_probe_batch_create_invalid_id = namespace.urn("RangeProbe", "batch-create-invalid")?;
    let range_probe_batch_upsert_valid_id = namespace.urn("RangeProbe", "batch-upsert-valid")?;
    let range_probe_batch_upsert_invalid_id = namespace.urn("RangeProbe", "batch-upsert-invalid")?;
    let range_probe_batch_create_valid = Entity::builder()
        .id(range_probe_batch_create_valid_id.clone())
        .entity_type(range_probe_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    let document = UncheckedDocument::from_entity(&range_probe_batch_create_valid, Lossless::Normalized);
    let range_probe_batch_create_invalid = Entity::builder()
        .id(range_probe_batch_create_invalid_id.clone())
        .entity_type(range_probe_type.clone())
        .build()?;
    // The model refuses it (a position the constructor refuses: latitude 91 lies outside [-90, 90]), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, 91]}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&range_probe_batch_create_invalid, Lossless::Normalized).set_member("location", &location);
    let entities = [range_probe_batch_create_valid, range_probe_batch_create_invalid];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&range_probe_batch_create_valid_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(
            0,
            batch_error()
                .entity(&range_probe_batch_create_invalid_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("91")
        )
    );
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_batch_create_valid_id)
        .send()
        .await?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_batch_create_invalid_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let range_probe_batch_upsert_valid = Entity::builder()
        .id(range_probe_batch_upsert_valid_id.clone())
        .entity_type(range_probe_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    let document_3 = UncheckedDocument::from_entity(&range_probe_batch_upsert_valid, Lossless::Normalized);
    let range_probe_batch_upsert_invalid = Entity::builder()
        .id(range_probe_batch_upsert_invalid_id.clone())
        .entity_type(range_probe_type.clone())
        .build()?;
    // The model refuses it (a position the constructor refuses: latitude 91 lies outside [-90, 90]), so `location` is written as text.
    let location_2: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, 91]}}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&range_probe_batch_upsert_invalid, Lossless::Normalized).set_member("location", &location_2);
    let entities_2 = [range_probe_batch_upsert_valid, range_probe_batch_upsert_invalid];
    let request_2 = broker
        .unchecked(&BatchUpsert::builder().entities(&entities_2).build())?
        .documents(&[document_3, document_4]);
    let answer_3 = broker.send_unchecked(request_2).await;
    let report_2: BatchOperationResult = reported(answer_3)?;
    expect_that!(&report_2.success, len(eq(1)));
    expect_that!(&report_2.success, nth(0, eq(&range_probe_batch_upsert_valid_id)));
    expect_that!(&report_2.errors, len(eq(1)));
    expect_that!(
        &report_2.errors,
        nth(
            0,
            batch_error()
                .entity(&range_probe_batch_upsert_invalid_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("91")
        )
    );
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_batch_upsert_valid_id)
        .send()
        .await?;
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_batch_upsert_invalid_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    Ok(())
}
