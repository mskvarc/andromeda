//! A query pages through the Context Source Registration Subscriptions of the tenant, and the `Link` field of each page
//! names the pages before and after it (clause 5.11.5).

use crate::{
    pagination_links::{PageLink, expect_page_links, page_link},
    registration_subscription_operations::create_registration_subscription_triple,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::RelationType,
    model::{APPLICATION_LD_JSON, NgsiLdMediaType},
};
use serde_json::{Value, json};

/// Pins: Check that one can query context source registration subscriptions with providing offset and limit
/// parameters for pagination
/// Clause: 5.11.5
/// Expected: page links compare by their limit and offset only (clauses 6.3.10, 5.5.9.2)
/// Expected: the registration subscription pagination query requests application/ld+json (clause 6.3.6)
/// Expected: the pagination links target real pages, and only the relations clause 5.5.9 defines are checked
/// Tenant: private (Query Context Source Registration Subscriptions pages through every context source registration
/// subscription of the tenant, and the case checks how many a limit and an offset let through and the prev and next
/// links that follow from the tenant's total; in the shared tenant every other file's subscriptions are paged too, so
/// only a tenant of its own keeps the page and its links exact)
#[gtest]
#[tokio::test]
async fn e041_03_01_query_second_subscription() -> Result<()> {
    paged(
        1,
        1,
        expect_one_subscription,
        &[
            page_link("/ngsi-ld/v1/csourceSubscriptions?limit=1&offset=0", RelationType::Prev, &APPLICATION_LD_JSON)?,
            page_link("/ngsi-ld/v1/csourceSubscriptions?limit=1&offset=2", RelationType::Next, &APPLICATION_LD_JSON)?,
        ],
    )
    .await
}

/// Pins: Check that one can query context source registration subscriptions with providing offset and limit
/// parameters for pagination
/// Clause: 5.11.5
/// Expected: page links compare by their limit and offset only (clauses 6.3.10, 5.5.9.2)
/// Expected: the registration subscription pagination query requests application/ld+json (clause 6.3.6)
/// Expected: the pagination links target real pages, and only the relations clause 5.5.9 defines are checked
/// Tenant: private (Query Context Source Registration Subscriptions pages through every context source registration
/// subscription of the tenant, and the case checks how many a limit and an offset let through and the prev and next
/// links that follow from the tenant's total; in the shared tenant every other file's subscriptions are paged too, so
/// only a tenant of its own keeps the page and its links exact)
#[gtest]
#[tokio::test]
async fn e041_03_02_query_last_subscription() -> Result<()> {
    paged(
        2,
        2,
        expect_one_subscription,
        &[page_link(
            "/ngsi-ld/v1/csourceSubscriptions?limit=2&offset=0",
            RelationType::Prev,
            &APPLICATION_LD_JSON,
        )?],
    )
    .await
}

/// Pins: Check that one can query context source registration subscriptions with providing offset and limit
/// parameters for pagination
/// Clause: 5.11.5
/// Expected: page links compare by their limit and offset only (clauses 6.3.10, 5.5.9.2)
/// Expected: the registration subscription pagination query requests application/ld+json (clause 6.3.6)
/// Expected: the pagination links target real pages, and only the relations clause 5.5.9 defines are checked
/// Tenant: private (Query Context Source Registration Subscriptions pages through every context source registration
/// subscription of the tenant, and the case checks how many a limit and an offset let through and the prev and next
/// links that follow from the tenant's total; in the shared tenant every other file's subscriptions are paged too, so
/// only a tenant of its own keeps the page and its links exact)
#[gtest]
#[tokio::test]
async fn e041_03_03_query_all_subscriptions() -> Result<()> {
    paged(
        15,
        0,
        |body| {
            expect_that!(body.as_array().map(Vec::len), some(eq(3)));
            for pointer in ["/0/type", "/1/type", "/2/type"] {
                expect_that!(body.pointer(pointer), some(eq(&json!("Subscription"))));
            }
        },
        &[],
    )
    .await
}

/// Requires the body to be a list holding exactly one Subscription, or that Subscription as a single object.
fn expect_one_subscription(body: &Value) {
    let listed = body.as_array().map_or_else(|| vec![body], |items| items.iter().collect::<Vec<_>>());
    expect_that!(listed.len(), eq(1));
    expect_that!(
        listed.first().and_then(|subscription| subscription.get("type")),
        some(eq(&json!("Subscription")))
    );
}

/// Sets up the three subscriptions in a tenant of the test's own, queries them with `limit` and `offset` in the
/// vocabulary's `Link` context with `Accept: application/ld+json`, requires 200, checks the body with `check` and the
/// page links against `links`, and deletes the subscriptions.
async fn paged(limit: u32, offset: u32, check: impl FnOnce(&Value), links: &[PageLink]) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let (vocabulary, ids, _subscriptions) = create_registration_subscription_triple(&tenant, &namespace, &sink.endpoint(1)?).await?;

    let received = tenant
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .registration_subscriptions()
        .query()
        .limit(limit)
        .offset(offset)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    check(&json_body(&received)?);
    expect_page_links(received.head.headers(), links)?;

    for id in &ids {
        tenant.registration_subscriptions().delete(id).send().await.any_answer();
    }
    Ok(())
}
