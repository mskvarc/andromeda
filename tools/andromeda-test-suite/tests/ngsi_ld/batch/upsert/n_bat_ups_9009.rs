//! An update mode upsert deletes with the null and refuses it on create.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        UncheckedDocument,
        headers::Accept,
        operation::{BatchUpsert, UpsertMode},
    },
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Batch Entity Upsert in update mode setting a Property value to urn:ngsi-ld:null deletes it from an
/// existing Entity and refuses, as one `BadRequestData` error, the same payload for an Entity that does not exist,
/// which is not created
/// Clause: 5.6.8.4, 5.6.8.5, 5.6.2.4, 5.6.1, 5.2.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_9009_an_update_mode_upsert_deletes_with_the_null_and_refuses_it_on_create() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_upsert_update_existing_id = namespace.urn("Building", "upsert-update-existing")?;
    let building_type = namespace.entity_type("Building")?;
    let building_upsert_update_fresh_id = namespace.urn("Building", "upsert-update-fresh")?;
    let building_upsert_update_existing = Entity::builder()
        .id(building_upsert_update_existing_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .property("humidity", 40)
        .build()?;
    broker.entities().create(&building_upsert_update_existing).send().await?.done()?;
    let building_upsert_update_existing_2 = Entity::builder()
        .id(building_upsert_update_existing_id.clone())
        .entity_type(building_type.clone())
        .property("humidity", 55)
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at temperature), so `temperature` is written as text.
    let temperature: RawJson = r#"{"type": "Property", "value": "urn:ngsi-ld:null"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_upsert_update_existing_2, Lossless::Normalized).set_member("temperature", &temperature);
    let building_upsert_update_fresh = Entity::builder()
        .id(building_upsert_update_fresh_id.clone())
        .entity_type(building_type.clone())
        .property("humidity", 60)
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at temperature), so `temperature` is written as text.
    let temperature_2: RawJson = r#"{"type": "Property", "value": "urn:ngsi-ld:null"}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_upsert_update_fresh, Lossless::Normalized).set_member("temperature", &temperature_2);
    let entities = [building_upsert_update_existing_2, building_upsert_update_fresh];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&entities).mode(UpsertMode::Update).build())?
        .documents(&[document, document_2]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_upsert_update_existing_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(0, batch_error().entity(&building_upsert_update_fresh_id).problem(ErrorType::BadRequestData))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_update_existing_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("temperature"));
    expect_that!(entity, has_attribute("humidity", any_attribute().value(55)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_update_fresh_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
