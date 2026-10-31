//! The `numberOfHits` of an `ImplicitlyCreated` `@context` counts the requests that use it (clauses 5.13.3.5 and
//! 5.13.4).

use crate::jsonld_context::{
    implicit_context::{ImplicitSubscription, ImplicitTeardown},
    stored_contexts::{detailed_listing, served_details},
    testing_entity::{post_in_context, testing_entity},
};
use andromeda_test_suite::{broker, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::http::operation::{ContextKindFilter, ServeContextDetails};

/// Pins: Check that the numberOfHits is increased after using a `ImplicitlyCreated` context
/// Clause: 5.13.4
/// Expected: numberOfHits counts uses of the @context, not serves of its metadata (clause 5.13.3.5)
/// Isolation: the remote @contexts are served per file under the mock origin, at `contexts/<ns>/` followed by their
/// host and path, as application/ld+json documents mapping the terms the test uses to per-file IRIs
/// Isolation: the subscription id carries the namespace and its endpoint is the test's own notification path; no entity
/// of type T1 is written, so no notification is sent
/// Expected: a Subscription has no numberOfHits member (Table 5.2.12-1), so the answer carries none
/// Expected: numberOfHits is absent or the expected number (clause 5.13.3.5 makes it a number)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
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
    let details = served_details(&broker, &implicit.id).await?;
    expect_that!(details.number_of_hits, any![none(), some(eq(1))]);
    implicit.delete(&broker, ImplicitTeardown::Delete).await?;
    broker.entities().delete(&testing.id).send().await.any_answer();
    broker.temporal().delete(&testing.id).send().await.any_answer();
    Ok(())
}
