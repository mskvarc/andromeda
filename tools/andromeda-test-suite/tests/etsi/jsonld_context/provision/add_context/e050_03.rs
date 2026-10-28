//! ETSI `050_03`: a subscription whose `@context` lists remote documents makes the broker create one
//! `ImplicitlyCreated` `@context` (clause 5.13.2).

use crate::jsonld_context::{
    implicit_context::ImplicitSubscription,
    stored_contexts::{detailed_listing, listed_id, sole_entry_besides_core},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::http::operation::ContextKindFilter;
use serde_json::json;

/// Pins: Check that one can create a subscription
/// Clause: 5.13.2
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/` followed by their host and path, as application/ld+json documents mapping the
/// terms the case uses to per-file IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: the case takes the first entry of the `ImplicitlyCreated` listing as its own, but clause 5.13.3 gives the
/// listing no order and Andromeda lists its embedded core @context among the `ImplicitlyCreated` entries of every
/// tenant; the port takes the one entry other than the core @context and checks there is exactly one
/// Tenant: private (the case takes the `ImplicitlyCreated` entry its subscription creates as its own and its teardown
/// deletes it; in the shared tenant other files' entries are listed beside it)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e050_03_01_check_the_creation_of_an_implicitly_created_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let subscription = ImplicitSubscription::create(&tenant, &namespace).await?;
    let listing = detailed_listing(&tenant, Some(ContextKindFilter::ImplicitlyCreated)).await?;
    let created = sole_entry_besides_core(&listing)?;
    expect_that!(created.get("kind"), some(eq(&json!("ImplicitlyCreated"))));
    subscription.delete(&tenant, Some(&listed_id(created)?)).await
}
