//! ETSI `001_05`: an entity created as `application/json` with no `Link` header gets the default `@context`, and keeps
//! the attribute the default context names, whether or not the retrieval requests a context (clause 6.3.5).

use crate::{
    suite_keywords::{create_entity_plain, delete_entities, non_empty_member, retrieve_entity_json},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Check that the default @context is used if the Content-Type header is "application/json" and the Link header
/// does not contain a JSON-LD @context, requesting without context
/// Clause: 6.3.5
/// Changed: the create is required to answer 201, where the Hurl line accepts any status; the retrieval that follows
/// reads the entity the create made
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e001_05_01_default_context_with_json_content_type_retrieved_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entity = building_simple_attributes(&namespace, 1)?;
    create_entity_plain(&broker, &entity).await?;
    let body = retrieve_entity_json(&broker.with_accept(Accept::any()), &entity.id).await?;
    expect_that!(non_empty_member(&body, "almostFull"), some(anything()));
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that the default @context is used if the Content-Type header is "application/json" and the Link header
/// does not contain a JSON-LD @context, requesting with context
/// Clause: 6.3.5
/// Changed: the create is required to answer 201, where the Hurl line accepts any status; the retrieval that follows
/// reads the entity the create made
#[gtest]
#[tokio::test]
async fn e001_05_02_default_context_with_json_content_type_retrieved_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    create_entity_plain(&broker, &entity).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(non_empty_member(&body, "ngsi-ld:default-context/almostFull"), some(anything()));
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
