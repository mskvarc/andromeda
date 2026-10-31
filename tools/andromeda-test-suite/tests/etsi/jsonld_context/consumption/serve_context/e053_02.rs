//! A hosted `@context` served with `details=true` answers its details, in `application/json`, with no member beyond
//! them (clauses 5.13.3.5 and 5.13.4).

use crate::jsonld_context::stored_contexts::{add_context, attribute_terms, expect_context_details, served_details};
use andromeda_test_suite::{broker, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::model::StoredContextKind;

/// Pins: Check that one can serve a @context with details
/// Clause: 5.13.4
/// Expected: lastUsage and numberOfHits are absent or of their type (a `DateTime` and a number, clause 5.13.3.5)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e053_02_01_serve_a_context_with_details_set_to_true() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = add_context(&broker, &attribute_terms(&namespace, &["A1", "A2", "A3"])?).await?;
    let details = served_details(&broker, &added.id).await?;
    expect_context_details(&details, &StoredContextKind::Hosted);
    broker.contexts().delete(&added.id).send().await.any_answer();
    Ok(())
}
