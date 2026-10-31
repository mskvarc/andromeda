//! The Buildings the attribute and ordering tests set up before they run and delete after.

use crate::{
    building_payloads::{BUILDING_LOCATED_STREET_TERMS, building_located_street_geo, building_two_instances},
    entity_operations::{create_entity, create_entity_with_context_string, delete_entities},
};
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input, Property};
use serde_json::{Value, json};

/// The setup of the attribute deletion tests: registers the terms of the Building cases and creates the Building of
/// [`building_two_instances`] with `second` as the dataset of its second instances, in a `@context` string.
pub(crate) async fn create_building_instances(broker: &BrokerClient, namespace: &Namespace, second: Option<&str>) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    create_entity_with_context_string(broker, &vocabulary, &building_two_instances(namespace, second)?).await?;
    Ok(vocabulary)
}

/// Registers the terms of the Building cases and creates the Building `urn:ngsi-ld:Vehicle:<ns>-1` of
/// [`building_located_street_geo`] in a `@context` string, answering the vocabulary.
pub(crate) async fn create_building_located_street_geo(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let building = building_located_street_geo(namespace, namespace.urn("Vehicle", 1)?)?;
    create_entity_with_context_string(broker, &vocabulary, &building).await?;
    Ok(vocabulary)
}

/// The terms of the `@context` of the Building cases that order by a name and its sub-attribute.
pub(crate) const BUILDING_TERMS: [&str; 3] = ["Building", "name", "subProperty"];

/// The Building `urn:ngsi-ld:Building:<ns>-<n>`, named `name` with a `subProperty` of value `sub`.
pub(crate) fn named_building(namespace: &Namespace, n: u32, name: &str, sub: u32) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", n)?)
        .entity_type("Building")
        .property("name", Property::builder(name).property("subProperty", Property::builder(sub)))
        .build()?)
}

/// Creates the three Buildings in the order 2, 1, 3: named B, A and C, with sub-properties 2, 1 and 3.
pub(crate) async fn create_named_buildings(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &named_building(namespace, 2, "B", 2)?).await?;
    create_entity(broker, vocabulary, &named_building(namespace, 1, "A", 1)?).await?;
    create_entity(broker, vocabulary, &named_building(namespace, 3, "C", 3)?).await
}

/// The teardown of the Building cases: deletes the three Buildings of [`create_named_buildings`].
pub(crate) async fn delete_named_buildings(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_entities(
        broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Expects the answer to hold exactly the three Buildings numbered `order`, from the first position to the last.
pub(crate) fn expect_building_order(body: &Value, namespace: &Namespace, order: [u32; 3]) {
    expect_that!(body.as_array().map(Vec::len), some(eq(3)));
    for (position, n) in order.into_iter().enumerate() {
        expect_that!(body.pointer(&format!("/{position}/id")), some(eq(&json!(namespace.urn_text("Building", n)))));
    }
}
