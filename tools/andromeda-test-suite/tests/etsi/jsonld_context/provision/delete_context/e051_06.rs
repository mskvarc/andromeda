//! ETSI `051_06`: an `ImplicitlyCreated` `@context` is deleted with 204 and an empty body when no `reload` is given
//! (clause 5.13.5).

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
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/` followed by their host and path, as application/ld+json documents mapping the
/// terms the case uses to per-file IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a Subscription has no such member, Table 5.2.12-1), so its check never applies; the port requires
/// the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
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
