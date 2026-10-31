//! A query specifying a maximum number of results answers at most that many entities (clause 6.3.10).

use crate::{
    building_payloads::bare_building,
    entity_operations::{create_entity, delete_entities},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;
use serde_json::{Value, json};

/// Pins: Check that one can query entities specifying a maximum number of results
/// Clause: 6.3.10
#[gtest]
#[tokio::test]
async fn e019_06_01_query_entities_specifying_a_maximum_number_of_results() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    for n in 1..=3 {
        create_entity(&broker, &vocabulary, &bare_building(&namespace, n)?).await?;
    }
    let received = broker
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .ids(vec![
            namespace.urn("Building", 1)?,
            namespace.urn("Building", 2)?,
            namespace.urn("Building", 3)?,
        ])
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .limit(2)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body: Value = json_body(&received?)?;
    expect_that!(body.as_array().map(Vec::len), some(eq(2)));
    expect_that!(body.pointer("/0/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    expect_that!(body.pointer("/1/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}
