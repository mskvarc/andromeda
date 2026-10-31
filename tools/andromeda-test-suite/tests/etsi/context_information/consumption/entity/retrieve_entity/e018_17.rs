//! An entity is retrieved with `pick` or `omit` on its attributes, on its core members, or on both (clauses 5.7.1 and
//! 4.21).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    entity_operations::{OBSERVED_AT_WHOLE_SECOND, create_entity, delete_entities, observed_at},
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
use ngsi_ld::{
    model::{EntityId, NgsiLdMediaType},
    query::projection::{self, EntityMember, Projection},
};
use serde_json::{Value, json};

/// Creates the case's Building in a `@context` of its own, which the answer is read with.
async fn created_building(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity(broker, &vocabulary, &building_simple_attributes(namespace, 1)?).await?;
    Ok(vocabulary)
}

/// Retrieves `id` with `pick` or `omit` applied, as `application/json` in `vocabulary`'s context, requires 200 and
/// answers the body.
async fn retrieved(broker: &BrokerClient, vocabulary: &Vocabulary, id: &EntityId, pick: Option<Projection>, omit: Option<Projection>) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .retrieve_projected(id)
        .maybe_pick(pick)
        .maybe_omit(omit)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can retrieve an entity with pick on attributes
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_17_01_retrieve_with_pick_on_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let pick = projection::attributes(["name"]).build()?;
    let body = retrieved(&broker, &vocabulary, &id, Some(pick), None).await?;
    let expected = json!({"name": {"type": "Property", "value": "Eiffel Tower"}});
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can retrieve an entity with omit on attributes
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_17_02_retrieve_with_omit_on_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let omit = projection::attributes(["name"]).build()?;
    let body = retrieved(&broker, &vocabulary, &id, None, Some(omit)).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Building",
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {"type": "Property", "value": 4, "unitCode": "C62", "observedAt": "2020-09-09T16:40:00.000Z"},
        "almostFull": {"type": "Property", "value": false},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(observed_at(&body, "airQualityLevel"), some(matches_regex(OBSERVED_AT_WHOLE_SECOND)));
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can retrieve an entity with pick on core members
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_17_03_retrieve_with_pick_on_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let pick = projection::member(EntityMember::Id).build()?;
    let body = retrieved(&broker, &vocabulary, &id, Some(pick), None).await?;
    let expected = json!({"id": id.to_string()});
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can retrieve an entity with omit on core members
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_17_04_retrieve_with_omit_on_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let omit = projection::member(EntityMember::Id).build()?;
    let body = retrieved(&broker, &vocabulary, &id, None, Some(omit)).await?;
    let expected = json!({
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {"type": "Property", "value": 4, "unitCode": "C62", "observedAt": "2020-09-09T16:40:00.000Z"},
        "almostFull": {"type": "Property", "value": false},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(observed_at(&body, "airQualityLevel"), some(matches_regex(OBSERVED_AT_WHOLE_SECOND)));
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can retrieve an entity with pick on attributes and core members
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_17_05_retrieve_with_pick_on_attributes_and_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let pick = projection::member(EntityMember::Id).attribute("name").build()?;
    let body = retrieved(&broker, &vocabulary, &id, Some(pick), None).await?;
    let expected = json!({
        "id": id.to_string(),
        "name": {"type": "Property", "value": "Eiffel Tower"},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can retrieve an entity with omit on attributes and core members
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_17_06_retrieve_with_omit_on_attributes_and_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let omit = projection::member(EntityMember::Type).attribute("name").build()?;
    let body = retrieved(&broker, &vocabulary, &id, None, Some(omit)).await?;
    let expected = json!({
        "id": id.to_string(),
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {"type": "Property", "value": 4, "unitCode": "C62", "observedAt": "2020-09-09T16:40:00.000Z"},
        "almostFull": {"type": "Property", "value": false},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(observed_at(&body, "airQualityLevel"), some(matches_regex(OBSERVED_AT_WHOLE_SECOND)));
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
