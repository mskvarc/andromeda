//! A partial update cannot delete a dataset id.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateAttributes},
    model::{Attribute, AttributeName, DatasetId, Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson, Value},
};

/// Pins: Update Attributes and Partial Attribute Update whose fragment sets `datasetId` to urn:ngsi-ld:null each
/// answer 400 `BadRequestData` and leave the named instance as it was
/// Clause: 5.5.8, 5.5.4, 5.6.2, 5.6.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9011_a_partial_update_cannot_delete_a_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_partial_update_dataset_id_id = namespace.urn("Building", "partial-update-dataset-id")?;
    let building_type = namespace.entity_type("Building")?;
    let a_dataset = namespace.dataset_id("Dataset", "A")?;
    let building_partial_update_dataset_id = Entity::builder()
        .id(building_partial_update_dataset_id_id.clone())
        .entity_type(building_type)
        .property(
            "temperature",
            Property::builder(Value::Number("21.5".parse()?)).dataset_id(namespace.dataset_id("Dataset", "A")?),
        )
        .build()?;
    broker.entities().create(&building_partial_update_dataset_id).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Property","value":9,"datasetId":"urn:ngsi-ld:null"}), so `temperature` is written as text.
    let temperature: RawJson = r#"{"type": "Property", "value": 9, "datasetId": "urn:ngsi-ld:null"}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("temperature", &temperature);
    let request = broker
        .unchecked(
            &UpdateAttributes::builder()
                .id(&building_partial_update_dataset_id_id)
                .fragment(&fragment)
                .build(),
        )?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let patch = Attribute::from(Property::builder(9).dataset_id("urn:ngsi-ld:null".parse::<DatasetId>()?).build()?).into_patch();
    let answer_2 = broker
        .with_responses()
        .entities()
        .update_attribute(&building_partial_update_dataset_id_id, &"temperature".parse::<AttributeName>()?, patch)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_partial_update_dataset_id_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("temperature", any_attribute().with_dataset_id(a_dataset).value(Value::Number("21.5".parse()?)))
    );
    Ok(())
}
