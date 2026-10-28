//! ETSI `018_11`: an entity whose Relationship targets a linked entity is retrieved with that entity inline, and with
//! the system attributes of the linked entity (clauses 5.7.1 and 4.5.23).

use crate::{
    suite_keywords::create_entity,
    suite_payloads::{building_located_at, city_paris},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{InlineJoin, SystemAttributes},
    model::EntityId,
};
use serde_json::Value;
use std::num::NonZeroU32;

/// The terms of the case's `@context`.
const TERMS: [&str; 3] = ["Building", "locatedAt", "City"];

/// Deletes `building` and `city` from the entity and temporal stores, as the case's teardown does.
async fn deleted(broker: &BrokerClient, building: &EntityId, city: &EntityId) {
    broker.entities().delete(building).send().await.any_answer();
    broker.temporal().delete(building).send().await.any_answer();
    broker.entities().delete(city).send().await.any_answer();
    broker.temporal().delete(city).send().await.any_answer();
}

/// Pins: Check that an entity can be retrieved with an inline linked entity with sysAttrs
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_11_01_retrieve_entity_with_inline_linked_entity_and_sys_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_entity(&broker, &vocabulary, &building_located_at(&namespace, "018-11")?).await?;
    create_entity(&broker, &vocabulary, &city_paris(&namespace)?).await?;
    let building = namespace.urn("Building", "018-11")?;
    let city = namespace.urn("City", "Paris")?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve(&building)
        .system_attributes(SystemAttributes::Include)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body: Value = serde_json::from_slice(&received?.body)?;
    expect_that!(body.pointer("/locatedAt/entity/createdAt"), some(anything()));
    deleted(&broker, &building, &city).await;
    Ok(())
}
