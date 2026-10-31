//! A merge cannot delete a dataset id.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::MergeEntity},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson, Value},
};

/// Pins: Merge Entity whose fragment sets an instance `datasetId` to urn:ngsi-ld:null answers 400 `BadRequestData`
/// and leaves the named instance as it was
/// Clause: 5.5.12, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9003_a_merge_cannot_delete_a_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_merge_dataset_id_id = namespace.urn("Building", "merge-dataset-id")?;
    let building_type = namespace.entity_type("Building")?;
    let a_dataset = namespace.dataset_id("Dataset", "A")?;
    let building_merge_dataset_id = Entity::builder()
        .id(building_merge_dataset_id_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(Value::Number("21.5".parse()?)).dataset_id(namespace.dataset_id("Dataset", "A")?),
        )
        .build()?;
    broker.entities().create(&building_merge_dataset_id).send().await?.done()?;
    let fragment = EntityFragment::for_entity(building_merge_dataset_id_id.clone(), building_type.clone()).build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Property","value":9,"datasetId":"urn:ngsi-ld:null"}), so `temperature` is written as text.
    let temperature: RawJson = r#"{"type": "Property", "value": 9, "datasetId": "urn:ngsi-ld:null"}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("temperature", &temperature);
    let request = broker
        .unchecked(&MergeEntity::builder().id(&building_merge_dataset_id_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_dataset_id_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("temperature", any_attribute().with_dataset_id(a_dataset).value(Value::Number("21.5".parse()?)))
    );
    Ok(())
}
