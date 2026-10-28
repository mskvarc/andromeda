//! ETSI `051_07`: a reload of an `ImplicitlyCreated` `@context` is refused with 400 `BadRequestData` (clause 5.13.5).

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
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/` followed by their host and path, as application/ld+json documents mapping the
/// terms the case uses to per-file IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: the case takes the first entry of the `ImplicitlyCreated` listing as its own, but clause 5.13.3 gives the
/// listing no order and Andromeda lists its embedded core @context among the `ImplicitlyCreated` entries of every
/// tenant; the port takes the one entry other than the core @context and checks there is exactly one
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Tenant: private (the case takes the `ImplicitlyCreated` entry its subscription creates as its own and its teardown
/// deletes it; in the shared tenant other files' entries are listed beside it)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
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
