//! A Purge Entities that drops an attribute removes it from the matching entities and keeps the entities (clauses
//! 5.6.21 and 6.4.3.3).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    entity_operations::{create_entity, delete_entities},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::AttributeName;

/// Pins: Check that purge entities with drop parameter removes only the specified attributes while keeping the entities
/// Clause: 5.6.21, 6.4.3.3
#[gtest]
#[tokio::test]
async fn e060_04_01_purge_entities_drop_specific_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let original = building_simple_attributes(&namespace, "060-05-1")?;
    create_entity(&broker, &vocabulary, &original).await?;
    let client = broker.with_context(vocabulary.context())?;
    let purged = client
        .with_responses()
        .entities()
        .purge()
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .drop(vec!["name".parse::<AttributeName>()?])
        .send()
        .await;
    verify_that!(&purged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(purged?.body.len(), eq(0));
    let received = client.with_responses().entities().retrieve(&original.id).send().await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.get("name"), none());
    expect_that!(body.get("subCategory"), some(anything()));
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
