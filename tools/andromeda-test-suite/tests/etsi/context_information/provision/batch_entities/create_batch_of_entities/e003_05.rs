//! A batch sent as `application/json` without a `Link` header is created in the default context, and the entity is
//! retrieved with and without a context (clause 6.3.5).

use crate::{
    batch_operations::batch_create,
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    entity_operations::{delete_entities, non_empty_member, retrieve_entity_json},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: Check that the default @context is used if the Content-Type header is "application/json" and the Link header
/// does not contain a JSON-LD @context and retrieve the entity without context
/// Clause: 6.3.5
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e003_05_01_default_context_with_json_content_type_retrieved_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entities = [building_simple_attributes(&namespace, 1)?];
    batch_create(&broker, &entities).await?;
    let reader = broker.with_accept(Accept::any());
    let body = retrieve_entity_json(&reader, &namespace.urn("Building", 1)?).await?;
    expect_that!(non_empty_member(&body, "almostFull"), some(anything()));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}

/// Pins: Check that the default @context is used if the Content-Type header is "application/json" and the Link header
/// does not contain a JSON-LD @context and retrieve the entity with context
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e003_05_02_default_context_with_json_content_type_retrieved_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entities = [building_simple_attributes(&namespace, 1)?];
    batch_create(&broker, &entities).await?;
    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = retrieve_entity_json(&reader, &namespace.urn("Building", 1)?).await?;
    expect_that!(non_empty_member(&body, "ngsi-ld:default-context/almostFull"), some(anything()));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}
