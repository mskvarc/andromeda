//! ETSI `006_01`: a batch of two entities is deleted by its ids, and the broker no longer retrieves them (clause 5.6.10).

use crate::{
    suite_keywords::{create_entity_with_context_string, expect_batch_done, query_entities_json},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;
use serde_json::json;

/// Pins: Check that one can delete a batch of entities
/// Clause: 5.6.10
#[gtest]
#[tokio::test]
async fn e006_01_01_delete_a_batch_of_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let ids = [namespace.urn("Building", 1)?, namespace.urn("Building", 2)?];
    let answer = broker.with_responses().entities().batch_delete(&ids).send().await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = query_entities_json(&client, ids.into(), "Building").await?;
    expect_that!(&body, matches_document(json!([])));
    Ok(())
}
