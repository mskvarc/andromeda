//! A json null on an optional member is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with an attribute whose `datasetId`, `observedAt` or `unitCode` is null, or with location or
/// scope null, answers 400 `BadRequestData` and stores nothing
/// Clause: 4.5.0, 5.2.1, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9047_a_json_null_on_an_optional_member_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_dataset_id = namespace.urn("Building", "dataset")?;
    let building_type = namespace.entity_type("Building")?;
    let building_observed_id = namespace.urn("Building", "observed")?;
    let building_unit_id = namespace.urn("Building", "unit")?;
    let building_location_id = namespace.urn("Building", "location")?;
    let building_scope_id = namespace.urn("Building", "scope")?;
    let building_dataset = Entity::builder().id(building_dataset_id).entity_type(building_type.clone()).build()?;
    // The model refuses it (`datasetId` must be a string at height), so `height` is written as text.
    let height: RawJson = r#"{"type": "Property", "value": 12, "datasetId": null}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_dataset, Lossless::Normalized).set_member("height", &height);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_dataset).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("datasetId")
    );
    let building_observed = Entity::builder().id(building_observed_id).entity_type(building_type.clone()).build()?;
    // The model refuses it (`observedAt` must be a string at height), so `height` is written as text.
    let height_2: RawJson = r#"{"type": "Property", "value": 12, "observedAt": null}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_observed, Lossless::Normalized).set_member("height", &height_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&building_observed).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_unit = Entity::builder().id(building_unit_id).entity_type(building_type.clone()).build()?;
    // The model refuses it (`unitCode` must be a string at height), so `height` is written as text.
    let height_3: RawJson = r#"{"type": "Property", "value": 12, "unitCode": null}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_unit, Lossless::Normalized).set_member("height", &height_3);
    let request_3 = broker.unchecked(&CreateEntity::builder().entity(&building_unit).build())?.document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_location = Entity::builder().id(building_location_id).entity_type(building_type.clone()).build()?;
    // The model refuses it (expected a JSON object at location), so `location` is written as text.
    let location: RawJson = "null".parse()?;
    let document_4 = UncheckedDocument::from_entity(&building_location, Lossless::Normalized).set_member("location", &location);
    let request_4 = broker
        .unchecked(&CreateEntity::builder().entity(&building_location).build())?
        .document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_scope = Entity::builder().id(building_scope_id).entity_type(building_type.clone()).build()?;
    // The model refuses it (`scope` must be a string), so `scope` is written as text.
    let scope: RawJson = "null".parse()?;
    let document_5 = UncheckedDocument::from_entity(&building_scope, Lossless::Normalized).set_member("scope", &scope);
    let request_5 = broker
        .unchecked(&CreateEntity::builder().entity(&building_scope).build())?
        .document(&document_5);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
