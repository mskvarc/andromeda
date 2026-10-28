//! ETSI `001_04`: an entity created with a `Link` header naming its `@context` and `application/json` is retrieved with
//! or without that context, and keeps the attribute the context names (clause 6.3.5).

use crate::{
    suite_keywords::{create_entity_with_link, delete_entities, non_empty_member, retrieve_entity_json},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
/// Clause: 6.3.5
/// Changed: the create is required to answer 201, where the Hurl line accepts any status; the retrieval that follows
/// reads the entity the create made
#[gtest]
#[tokio::test]
async fn e001_04_01_link_header_with_json_content_type_retrieved_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &entity).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(non_empty_member(&body, "almostFull"), some(anything()));
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
/// Clause: 6.3.5
/// Changed: the create is required to answer 201, where the Hurl line accepts any status; the retrieval that follows
/// reads the entity the create made
#[gtest]
#[tokio::test]
async fn e001_04_02_link_header_with_json_content_type_retrieved_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &entity).await?;
    let body = retrieve_entity_json(&broker.with_accept(Accept::any()), &entity.id).await?;
    let almost_full = namespace.expanded_term("almostFull")?;
    expect_that!(non_empty_member(&body, &almost_full), some(anything()));
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
