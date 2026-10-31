//! A hosted `@context` is deleted with 204 and an empty body when no `reload` is given (clause 5.13.5).

use crate::jsonld_context::stored_contexts::{add_context, attribute_terms};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Check that one can delete a hosted @context
/// Clause: 5.13.5
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e051_01_01_delete_a_context_whose_kind_is_hosted_without_reload_param() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = add_context(&broker, &attribute_terms(&namespace, &["A1", "A2", "A3"])?).await?;
    let deleted = broker.with_responses().contexts().delete(&added.id).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    expect_that!(deleted?.body, is_empty());
    Ok(())
}
