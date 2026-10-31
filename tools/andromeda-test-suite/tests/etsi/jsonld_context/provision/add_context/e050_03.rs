//! A subscription whose `@context` lists remote documents makes the broker create one `ImplicitlyCreated` `@context`
//! (clause 5.13.2).

use crate::jsonld_context::{
    implicit_context::ImplicitSubscription,
    stored_contexts::{detailed_listing, listed_id, sole_entry_besides_core},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::operation::ContextKindFilter, model::StoredContextKind};

/// Pins: Check that one can create a subscription
/// Clause: 5.13.2
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
async fn e050_03_01_check_the_creation_of_an_implicitly_created_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let subscription = ImplicitSubscription::create(&tenant, &namespace).await?;
    let listing = detailed_listing(&tenant, Some(ContextKindFilter::ImplicitlyCreated)).await?;
    let created = sole_entry_besides_core(&listing)?;
    expect_that!(created.kind, eq(&StoredContextKind::ImplicitlyCreated));
    subscription.delete(&tenant, Some(&listed_id(created)?)).await
}
