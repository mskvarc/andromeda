//! ETSI `060_05`: a Purge Entities with a `keep` parameter deletes every attribute but the kept ones and keeps the
//! entities (clauses 5.6.21 and 6.4.3.3).

use crate::{
    suite_keywords::{context_link, create_entity, delete_entities},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::{json_body, request_to},
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::LINK};
use url::form_urlencoded::Serializer;

/// Pins: Check that purge entities with keep parameter deletes all attributes except those specified while keeping the
/// entities
/// Clause: 5.6.21, 6.4.3.3
#[gtest]
#[tokio::test]
async fn e060_05_01_purge_entities_keep_only_specific_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let original = building_simple_attributes(&namespace, "060-06-1")?;
    create_entity(&broker, &vocabulary, &original).await?;
    // The typed purge refuses a selection of ids alone before sending, which the broker answers with 204.
    let query = Serializer::new(String::new())
        .append_pair("id", &namespace.urn_text("Building", "060-06-1"))
        .append_pair("keep", "name")
        .finish();
    let purge = request_to(&broker, Method::DELETE, &format!("entities/?{query}"))?.append_header(LINK, context_link(&vocabulary)?);
    let purged = broker.send_unchecked(purge).await;
    verify_that!(&purged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(purged?.body().len(), eq(0));
    let client = broker.with_context(vocabulary.context())?;
    let received = client.with_responses().entities().retrieve(&original.id).send().await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.get("name"), some(anything()));
    expect_that!(body.get("subCategory"), none());
    expect_that!(body.get("airQualityLevel"), none());
    expect_that!(body.get("almostFull"), none());
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
