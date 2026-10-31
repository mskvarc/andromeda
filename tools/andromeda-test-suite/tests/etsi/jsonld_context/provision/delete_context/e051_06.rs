//! An `ImplicitlyCreated` `@context` is deleted with 204 and an empty body when no `reload` is given (clause 5.13.5).

use crate::jsonld_context::{
    implicit_context::{ImplicitSubscription, ImplicitTeardown},
    stored_contexts::detailed_listing,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::operation::ContextKindFilter;

/// Pins: Check that one can delete a `ImplicitlyCreated` @context
/// Clause: 5.13.5
/// Isolation: the remote @contexts are served per file under the mock origin, at `contexts/<ns>/` followed by their
/// host and path, as application/ld+json documents mapping the terms the test uses to per-file IRIs
/// Isolation: the subscription id carries the namespace and its endpoint is the test's own notification path; no entity
/// of type T1 is written, so no notification is sent
/// Expected: a Subscription has no numberOfHits member (Table 5.2.12-1), so the answer carries none
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e051_06_01_delete_a_context_whose_kind_is_implicitly_created_without_reload_param() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subscription = ImplicitSubscription::create(&broker, &namespace).await?;
    detailed_listing(&broker, Some(ContextKindFilter::ImplicitlyCreated)).await?;
    let (implicit, retrieved) = subscription.retrieve(&broker).await?;
    expect_that!(retrieved.get("numberOfHits"), none());
    let deleted = broker.with_responses().contexts().delete(&implicit.id).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    expect_that!(deleted?.body, is_empty());
    implicit.delete(&broker, ImplicitTeardown::Keep).await
}
