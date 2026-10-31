//! A query pages through the subscriptions of the tenant, and the `Link` field of each page names the pages before and
//! after it (clause 5.8.4).

use crate::{
    pagination_links::{expect_page_links, page_link},
    subscription_operations::create_query_subscriptions,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::RelationType,
    model::{APPLICATION_LD_JSON, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: Check that one can query a list of subscriptions: Pagination logic shall be in place
/// Clause: 5.8.4
/// Expected: page links compare by their limit and offset only (clauses 6.3.10, 5.5.9.2)
/// Expected: the pagination links target real pages, and only the relations clause 5.5.9 defines are checked
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, and the case checks how many a
/// limit and an offset let through and the prev and next links that follow from the tenant's total; in the shared
/// tenant every other file's subscriptions are paged too, so only a tenant of its own keeps the page and its links
/// exact)
#[gtest]
#[tokio::test]
async fn e031_02_01_query_second_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let (vocabulary, ids, _subscriptions) = create_query_subscriptions(&tenant, &namespace, &sink.endpoint(1)?).await?;

    let received = tenant
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .subscriptions()
        .query()
        .limit(1)
        .offset(1)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    let body = json_body(&received)?;
    let listed = body.as_array().map_or_else(|| vec![&body], |items| items.iter().collect::<Vec<_>>());
    expect_that!(listed.len(), eq(1));
    expect_that!(
        listed.first().and_then(|subscription| subscription.get("type")),
        some(eq(&json!("Subscription")))
    );
    expect_page_links(
        received.head.headers(),
        &[
            page_link("/ngsi-ld/v1/subscriptions?limit=1&offset=0", RelationType::Prev, &APPLICATION_LD_JSON)?,
            page_link("/ngsi-ld/v1/subscriptions?limit=1&offset=2", RelationType::Next, &APPLICATION_LD_JSON)?,
        ],
    )?;

    for id in &ids {
        tenant.subscriptions().delete(id).send().await.any_answer();
    }
    Ok(())
}

/// Pins: Check that one can query a list of subscriptions: Pagination logic shall be in place
/// Clause: 5.8.4
/// Expected: page links compare by their limit and offset only (clauses 6.3.10, 5.5.9.2)
/// Expected: the pagination links target real pages, and only the relations clause 5.5.9 defines are checked
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, and the case checks how many a
/// limit and an offset let through and the prev and next links that follow from the tenant's total; in the shared
/// tenant every other file's subscriptions are paged too, so only a tenant of its own keeps the page and its links
/// exact)
#[gtest]
#[tokio::test]
async fn e031_02_02_query_last_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let (vocabulary, ids, _subscriptions) = create_query_subscriptions(&tenant, &namespace, &sink.endpoint(1)?).await?;

    let received = tenant
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .subscriptions()
        .query()
        .limit(1)
        .offset(2)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    let body = json_body(&received)?;
    let listed = body.as_array().map_or_else(|| vec![&body], |items| items.iter().collect::<Vec<_>>());
    expect_that!(listed.len(), eq(1));
    expect_that!(
        listed.first().and_then(|subscription| subscription.get("type")),
        some(eq(&json!("Subscription")))
    );
    expect_page_links(
        received.head.headers(),
        &[page_link(
            "/ngsi-ld/v1/subscriptions?limit=1&offset=1",
            RelationType::Prev,
            &APPLICATION_LD_JSON,
        )?],
    )?;

    for id in &ids {
        tenant.subscriptions().delete(id).send().await.any_answer();
    }
    Ok(())
}

/// Pins: Check that one can query a list of subscriptions: Pagination logic shall be in place
/// Clause: 5.8.4
/// Expected: page links compare by their limit and offset only (clauses 6.3.10, 5.5.9.2)
/// Expected: the pagination links target real pages, and only the relations clause 5.5.9 defines are checked
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, and the case checks how many a
/// limit and an offset let through and the prev and next links that follow from the tenant's total; in the shared
/// tenant every other file's subscriptions are paged too, so only a tenant of its own keeps the page and its links
/// exact)
#[gtest]
#[tokio::test]
async fn e031_02_03_query_all_subscriptions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let (vocabulary, ids, _subscriptions) = create_query_subscriptions(&tenant, &namespace, &sink.endpoint(1)?).await?;

    let received = tenant
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .subscriptions()
        .query()
        .limit(15)
        .offset(0)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    let body = json_body(&received)?;
    expect_that!(body.as_array().map(Vec::len), some(eq(3)));
    for pointer in ["/0/type", "/1/type", "/2/type"] {
        expect_that!(body.pointer(pointer), some(eq(&json!("Subscription"))));
    }
    expect_page_links(received.head.headers(), &[])?;

    for id in &ids {
        tenant.subscriptions().delete(id).send().await.any_answer();
    }
    Ok(())
}
