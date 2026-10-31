//! Attribute instances are selected by their `datasetId`, alone or with an attribute name, and the default instance is
//! selected with `@none` (clauses 4.5.5 and 5.7.2).

use crate::{
    entity_operations::{create_entity, delete_entities},
    vehicle_entity_payloads::dataset,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, DatasetId, Entity, Input, NgsiLdMediaType, Property, Relationship};
use serde_json::{Value, json};

/// The terms of the `@context` the cases of this file register.
const TERMS: [&str; 3] = ["Building", "name", "locatedAt"];

/// The Building `urn:ngsi-ld:Building:<ns>-1` of the cases. Its `name` has the default instance and one instance per
/// dataset; its `locatedAt` has the default instance, and one instance per dataset with the Ville of the French
/// dataset and the Ciudad of the Spanish one.
fn building_with_datasets(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .property("name", Property::builder("Tour Eiffel").dataset_id(dataset(namespace, "french-name")?))
        .property("name", Property::builder("Eiffelturm").dataset_id(dataset(namespace, "german-name")?))
        .relationship("locatedAt", Relationship::builder(namespace.urn("City", "Paris")?))
        .relationship(
            "locatedAt",
            Relationship::builder(namespace.urn("Ville", "Paris")?).dataset_id(dataset(namespace, "french-name")?),
        )
        .relationship(
            "locatedAt",
            Relationship::builder(namespace.urn("Ciudad", "París")?).dataset_id(dataset(namespace, "spanish-name")?),
        )
        .build()?)
}

/// Queries the Buildings of `namespace` under `vocabulary`'s context in the ld+json representation, with the attribute
/// names `attrs` and the dataset ids `dataset_ids` (empty selects every value), and answers the body.
async fn query_buildings(broker: &BrokerClient, vocabulary: &Vocabulary, attrs: Vec<AttributeName>, dataset_ids: Vec<DatasetId>) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .attrs(attrs)
        .dataset_ids(dataset_ids)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can filter attribute instances based on datasetId.
/// Clause: 4.5.5, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_12_01_filter_based_on_attrs_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_entity(&broker, &vocabulary, &building_with_datasets(&namespace)?).await?;
    let body = query_buildings(&broker, &vocabulary, vec!["name".parse()?], Vec::new()).await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "name": [
                {"type": "Property", "value": "Eiffel Tower"},
                {"type": "Property", "value": "Tour Eiffel", "datasetId": namespace.urn_text("Dataset", "french-name")},
                {"type": "Property", "value": "Eiffelturm", "datasetId": namespace.urn_text("Dataset", "german-name")},
            ],
            "@context": vocabulary.url().as_str(),
        }]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances based on datasetId.
/// Clause: 4.5.5, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_12_02_filter_based_on_dataset_id_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_entity(&broker, &vocabulary, &building_with_datasets(&namespace)?).await?;
    let body = query_buildings(&broker, &vocabulary, Vec::new(), vec![dataset(&namespace, "french-name")?]).await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "name": {"type": "Property", "value": "Tour Eiffel", "datasetId": namespace.urn_text("Dataset", "french-name")},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("Ville", "Paris"), "datasetId": namespace.urn_text("Dataset", "french-name")},
            "@context": vocabulary.url().as_str(),
        }]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances based on datasetId.
/// Clause: 4.5.5, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_12_03_filter_based_on_two_dataset_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_entity(&broker, &vocabulary, &building_with_datasets(&namespace)?).await?;
    let body = query_buildings(
        &broker,
        &vocabulary,
        Vec::new(),
        vec![dataset(&namespace, "french-name")?, dataset(&namespace, "spanish-name")?],
    )
    .await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "name": {"type": "Property", "value": "Tour Eiffel", "datasetId": namespace.urn_text("Dataset", "french-name")},
            "locatedAt": [
                {"type": "Relationship", "object": namespace.urn_text("Ville", "Paris"), "datasetId": namespace.urn_text("Dataset", "french-name")},
                {"type": "Relationship", "object": namespace.urn_text("Ciudad", "París"), "datasetId": namespace.urn_text("Dataset", "spanish-name")},
            ],
            "@context": vocabulary.url().as_str(),
        }]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances based on datasetId.
/// Clause: 4.5.5, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_12_04_filter_based_on_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_entity(&broker, &vocabulary, &building_with_datasets(&namespace)?).await?;
    let body = query_buildings(&broker, &vocabulary, Vec::new(), vec![DatasetId::DefaultInstance]).await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
            "@context": vocabulary.url().as_str(),
        }]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances based on datasetId.
/// Clause: 4.5.5, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_12_05_filter_based_on_attrs_and_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_entity(&broker, &vocabulary, &building_with_datasets(&namespace)?).await?;
    let body = query_buildings(&broker, &vocabulary, vec!["name".parse()?], vec![DatasetId::DefaultInstance]).await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "@context": vocabulary.url().as_str(),
        }]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances based on datasetId.
/// Clause: 4.5.5, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_12_06_filter_based_on_attrs_and_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_entity(&broker, &vocabulary, &building_with_datasets(&namespace)?).await?;
    let body = query_buildings(&broker, &vocabulary, vec!["name".parse()?], vec![dataset(&namespace, "german-name")?]).await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffelturm", "datasetId": namespace.urn_text("Dataset", "german-name")},
            "@context": vocabulary.url().as_str(),
        }]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances based on datasetId.
/// Clause: 4.5.5, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_12_07_filter_based_on_attrs_and_dataset_id_with_no_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_entity(&broker, &vocabulary, &building_with_datasets(&namespace)?).await?;
    let body = query_buildings(&broker, &vocabulary, vec!["name".parse()?], vec![dataset(&namespace, "spanish-name")?]).await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "@context": vocabulary.url().as_str(),
        }]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}
