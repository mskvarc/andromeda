//! ETSI `011_06`: an update appends the types it names to an entity: one type, two types, a type the entity already holds,
//! and a type beside one it holds (clauses 5.6.2 and 4.16).

use crate::{
    suite_keywords::{create_entity_with_link, delete_entities, ld_json_body_client, retrieve_entity_json, update_attributes},
    suite_payloads::vehicle_id_building,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{EntityFragment, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that type is appended if it is not yet in the target entity
/// Clause: 5.6.2, 4.16
#[gtest]
#[tokio::test]
async fn e011_06_01_append_one_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "Vehicle"]).await?;
    let id = namespace.urn("Vehicle", 1)?;
    create_entity_with_link(&broker, &vocabulary, &vehicle_id_building(&namespace)?).await?;
    let fragment = EntityFragment::builder().entity_type("Vehicle").build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": ["Building", "Vehicle"],
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that type is appended if it is not yet in the target entity
/// Clause: 5.6.2, 4.16
#[gtest]
#[tokio::test]
async fn e011_06_02_append_two_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "Vehicle", "Car"]).await?;
    let id = namespace.urn("Vehicle", 1)?;
    create_entity_with_link(&broker, &vocabulary, &vehicle_id_building(&namespace)?).await?;
    let fragment = EntityFragment::builder().entity_type(["Vehicle", "Car"]).build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": ["Building", "Vehicle", "Car"],
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that type is appended if it is not yet in the target entity
/// Clause: 5.6.2, 4.16
#[gtest]
#[tokio::test]
async fn e011_06_03_append_existing_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let id = namespace.urn("Vehicle", 1)?;
    create_entity_with_link(&broker, &vocabulary, &vehicle_id_building(&namespace)?).await?;
    let fragment = EntityFragment::builder().entity_type("Building").build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that type is appended if it is not yet in the target entity
/// Clause: 5.6.2, 4.16
#[gtest]
#[tokio::test]
async fn e011_06_04_append_one_type_and_one_existing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "Vehicle"]).await?;
    let id = namespace.urn("Vehicle", 1)?;
    create_entity_with_link(&broker, &vocabulary, &vehicle_id_building(&namespace)?).await?;
    let fragment = EntityFragment::builder().entity_type(["Building", "Vehicle"]).build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": ["Building", "Vehicle"],
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
