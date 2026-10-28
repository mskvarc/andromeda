//! ETSI `003_04`: a batch sent as `application/json` with its `@context` named by a `Link` header is created, and the
//! entity is retrieved with and without that context (clauses 6.3.5 and 6.3.4).

use crate::{
    suite_keywords::{batch_create, delete_entities, non_empty_member, retrieve_entity_json},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e003_04_01_link_header_with_json_content_type_retrieved_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entities = [building_simple_attributes(&namespace, 1)?];
    batch_create(&broker.with_context(vocabulary.context())?, &entities).await?;
    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = retrieve_entity_json(&reader, &namespace.urn("Building", 1)?).await?;
    expect_that!(non_empty_member(&body, "almostFull"), some(anything()));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}

/// Pins: Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e003_04_02_link_header_with_json_content_type_retrieved_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entities = [building_simple_attributes(&namespace, 1)?];
    batch_create(&broker.with_context(vocabulary.context())?, &entities).await?;
    let reader = broker.with_accept(Accept::any());
    let body = retrieve_entity_json(&reader, &namespace.urn("Building", 1)?).await?;
    let almost_full = namespace.expanded_term("almostFull")?;
    expect_that!(non_empty_member(&body, &almost_full), some(anything()));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?]).await;
    Ok(())
}
