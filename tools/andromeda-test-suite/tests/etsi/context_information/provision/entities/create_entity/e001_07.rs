//! An entity created as `application/ld+json` with its `@context` in the body is retrieved with or without that
//! context, and keeps the attribute the context names (clause 6.3.5).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    entity_operations::{create_entity, delete_entities, non_empty_member, retrieve_entity_json},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: Check that the @context is obtained from the request payload body itself if the Content-Type header is
/// "application/ld+json" and retrieve with ld context
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e001_07_01_payload_context_with_json_ld_content_type_retrieved_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &entity).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(non_empty_member(&body, "almostFull"), some(anything()));
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that the @context is obtained from the request payload body itself if the Content-Type header is
/// "application/ld+json" and retrieve without context
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e001_07_02_payload_context_with_json_ld_content_type_retrieved_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &entity).await?;
    let body = retrieve_entity_json(&broker.with_accept(Accept::any()), &entity.id).await?;
    let almost_full = namespace.expanded_term("almostFull")?;
    expect_that!(non_empty_member(&body, &almost_full), some(anything()));
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
