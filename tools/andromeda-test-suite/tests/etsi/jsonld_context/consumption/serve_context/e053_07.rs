//! ETSI `053_07`: the `ImplicitlyCreated` `@context` a subscription makes the broker create is served with its details,
//! in `application/json`, with no member beyond them (clauses 5.13.2.4 and 5.13.4).

use crate::jsonld_context::{
    implicit_context::{ImplicitContext, ImplicitTeardown},
    stored_contexts::{details_in_json, expect_context_details, expect_only_detail_members, served_details},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::http::operation::ContextKindFilter;

/// Pins: Check that one can serve a `ImplicitlyCreated` @context with details set to true
/// Clause: 5.13.4
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: lastUsage and numberOfHits are required to be absent or of their type (a `DateTime` and a number, clause
/// 5.13.3.5), where the keyword also skips a null or empty value
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e053_07_01_serve_an_implicitly_created_context_with_details_set_to_true() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let implicit = ImplicitContext::set_up(&broker, &namespace).await?;
    let details = details_in_json(&served_details(&broker, &implicit.id).await?)?;
    expect_context_details(&details, ContextKindFilter::ImplicitlyCreated);
    expect_only_detail_members(&details)?;
    implicit.delete(&broker, ImplicitTeardown::Delete).await
}
