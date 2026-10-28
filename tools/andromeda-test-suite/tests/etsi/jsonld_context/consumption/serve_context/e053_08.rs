//! ETSI `053_08`: the `numberOfHits` of an `ImplicitlyCreated` `@context` counts the requests that use it (clauses
//! 5.13.3.5 and 5.13.4).

use crate::jsonld_context::{
    implicit_context::{ImplicitSubscription, ImplicitTeardown},
    stored_contexts::{detailed_listing, served_details},
    testing_entity::{post_in_context, testing_entity},
};
use andromeda_test_suite::{broker, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::http::operation::{ContextKindFilter, ServeContextDetails};
use serde_json::{Value, json};

/// Pins: Check that the numberOfHits is increased after using a `ImplicitlyCreated` context
/// Clause: 5.13.4
/// Corrected: fix(numberofhits): count context uses, not metadata serves (5.13.3.5)
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a Subscription has no such member, Table 5.2.12-1), so its check never applies; the port requires
/// the member to be absent, which implies the check
/// Changed: numberOfHits is required to be absent or the expected number, where the keyword also skips a null or empty
/// value and compares a text as a number; clause 5.13.3.5 makes it a number
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e053_08_01_number_of_hits_increases_after_using_an_implicitly_created_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subscription = ImplicitSubscription::create(&broker, &namespace).await?;
    detailed_listing(&broker, Some(ContextKindFilter::ImplicitlyCreated)).await?;
    let (implicit, retrieved) = subscription.retrieve(&broker).await?;
    expect_that!(retrieved.get("numberOfHits"), none());
    let testing = testing_entity(&namespace, 1)?;
    post_in_context(&broker, &testing, &implicit.url).await?;
    broker
        .send_unchecked(broker.unchecked(&ServeContextDetails { id: &implicit.id })?)
        .await
        .any_answer();
    let details = serde_json::from_slice::<Value>(served_details(&broker, &implicit.id).await?.body())?;
    expect_that!(details.get("numberOfHits"), any![none(), some(eq(&json!(1)))]);
    implicit.delete(&broker, ImplicitTeardown::Delete).await?;
    broker.entities().delete(&testing.id).send().await.any_answer();
    broker.temporal().delete(&testing.id).send().await.any_answer();
    Ok(())
}
