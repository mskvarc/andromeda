//! An entity is retrieved by its identifier, whole or reduced to the attributes asked for (clause 5.7.1).
//!
//! The answer is compared exactly with the test's expectation file: the `instanceId` the expectation hides is allowed
//! in every attribute, and the `@context` may be any versioned core `@context`, alone or as a one-element list.

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    entity_operations::create_entity,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::{ContextRule, matches_document},
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, Entity, Input, NgsiLdMediaType};
use serde_json::{Value, json};

/// Creates the test's building, and answers it.
async fn created_building(broker: &BrokerClient, namespace: &Namespace) -> Result<Entity<Input>> {
    let vocabulary = Vocabulary::register(broker, namespace, &BUILDING_SIMPLE_TERMS).await?;
    let building = building_simple_attributes(namespace, 1)?;
    create_entity(broker, &vocabulary, &building).await?;
    Ok(building)
}

/// Retrieves `building` with no `@context` as `application/ld+json`, keeping only `attrs` when it names any, requires
/// 200 and answers the body.
async fn retrieved(broker: &BrokerClient, building: &Entity<Input>, attrs: Vec<AttributeName>) -> Result<Value> {
    let received = broker
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .retrieve(&building.id)
        .attrs(attrs)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can get an entity by id
/// Clause: 5.7.1
#[gtest]
#[tokio::test]
async fn e018_01_01_get_an_entity_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building = created_building(&broker, &namespace).await?;
    let body = retrieved(&broker, &building, Vec::new()).await?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": namespace.expanded_term("Building")?,
        namespace.expanded_term("name")?: {"type": "Property", "value": "Eiffel Tower"},
        namespace.expanded_term("subCategory")?: {"type": "Property", "value": "tourism"},
        namespace.expanded_term("airQualityLevel")?: {"type": "Property", "value": 4, "unitCode": "C62", "observedAt": "2020-09-09T16:40:00.000Z"},
        namespace.expanded_term("almostFull")?: {"type": "Property", "value": false},
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
    });
    expect_that!(
        &body,
        matches_document(expected).hiding("instanceId").context_rule(ContextRule::AnyCoreContextVersion)
    );
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can query some attributes from an entity
/// Clause: 5.7.1
#[gtest]
#[tokio::test]
async fn e018_01_02_query_some_attributes_from_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building = created_building(&broker, &namespace).await?;
    let attrs = vec![
        namespace.expanded_term("airQualityLevel")?.parse()?,
        namespace.expanded_term("subCategory")?.parse()?,
    ];
    let body = retrieved(&broker, &building, attrs).await?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": namespace.expanded_term("Building")?,
        namespace.expanded_term("subCategory")?: {"type": "Property", "value": "tourism"},
        namespace.expanded_term("airQualityLevel")?: {"type": "Property", "value": 4, "unitCode": "C62", "observedAt": "2020-09-09T16:40:00.000Z"},
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
    });
    expect_that!(
        &body,
        matches_document(expected).hiding("instanceId").context_rule(ContextRule::AnyCoreContextVersion)
    );
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
