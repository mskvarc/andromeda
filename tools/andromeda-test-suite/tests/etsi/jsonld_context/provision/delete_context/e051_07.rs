//! A reload of an `ImplicitlyCreated` `@context` is refused with 400 `BadRequestData` (clause 5.13.5).

use crate::jsonld_context::{
    implicit_context::ImplicitSubscription,
    stored_contexts::{detailed_listing, listed_id, sole_entry_besides_core},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{ContextDeletion, ContextKindFilter},
    model::ErrorType,
};

/// Pins: Check that one cannot delete a `ImplicitlyCreated` @context with reload set to true
/// Clause: 5.13.5
/// Isolation: the remote @contexts are served per file under the mock origin, at `contexts/<ns>/` followed by their
/// host and path, as application/ld+json documents mapping the terms the test uses to per-file IRIs
/// Isolation: the subscription id carries the namespace and its endpoint is the test's own notification path; no entity
/// of type T1 is written, so no notification is sent
/// Expected: clause 5.13.3 gives the `ImplicitlyCreated` listing no order and Andromeda lists its embedded core
/// @context among the `ImplicitlyCreated` entries of every tenant, so the test takes the one entry other than the core
/// @context and requires exactly one
/// Tenant: private (the case takes the `ImplicitlyCreated` entry its subscription creates as its own and its teardown
/// deletes it; in the shared tenant other files' entries are listed beside it)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e051_07_01_delete_an_implicitly_created_context_with_reload_set_to_true() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let subscription = ImplicitSubscription::create(&tenant, &namespace).await?;
    let listing = detailed_listing(&tenant, Some(ContextKindFilter::ImplicitlyCreated)).await?;
    let implicit = listed_id(sole_entry_besides_core(&listing)?)?;
    let reloaded = tenant
        .with_responses()
        .contexts()
        .delete(&implicit)
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(&reloaded, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail());
    subscription.delete(&tenant, Some(&implicit)).await
}
