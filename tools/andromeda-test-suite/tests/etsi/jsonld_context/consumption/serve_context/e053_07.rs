//! The `ImplicitlyCreated` `@context` a subscription makes the broker create is served with its details, in
//! `application/json`, with no member beyond them (clauses 5.13.2.4 and 5.13.4).

use crate::jsonld_context::{
    implicit_context::{ImplicitContext, ImplicitTeardown},
    stored_contexts::{expect_context_details, served_details},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::StoredContextKind;

/// Pins: Check that one can serve a `ImplicitlyCreated` @context with details set to true
/// Clause: 5.13.4
/// Isolation: the remote @contexts are served per file under the mock origin, at `contexts/<ns>/` followed by their
/// host and path, as application/ld+json documents mapping the terms the test uses to per-file IRIs
/// Isolation: the subscription id carries the namespace and its endpoint is the test's own notification path; no entity
/// of type T1 is written, so no notification is sent
/// Expected: lastUsage and numberOfHits are absent or of their type (a `DateTime` and a number, clause 5.13.3.5)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e053_07_01_serve_an_implicitly_created_context_with_details_set_to_true() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let implicit = ImplicitContext::set_up(&broker, &namespace).await?;
    let details = served_details(&broker, &implicit.id).await?;
    expect_context_details(&details, &StoredContextKind::ImplicitlyCreated);
    implicit.delete(&broker, ImplicitTeardown::Delete).await
}
