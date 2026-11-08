//! A subscription cursor with a forged query digest is refused.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    followed_link::request_replacing,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    problem::problem,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions with a cursor whose `pageQuery` is not the listing's digest answers 400 `BadRequestData`,
/// and the next pointer followed unchanged answers the third Subscription
/// Clause: 5.5.9.1, 6.3.20
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions its own)
#[gtest]
#[tokio::test]
async fn a_sub_qry_9084_a_subscription_cursor_with_a_forged_query_digest_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let building_type = namespace.entity_type("Building")?;
    let ids = [
        namespace.subscription_id("0000")?,
        namespace.subscription_id("0001")?,
        namespace.subscription_id("0002")?,
    ];
    for id in &ids {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([building_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.subscriptions().create(&subscription).send().await?;
    }
    let first = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .query()
        .limit(2)
        .send()
        .await?;
    expect_that!(first.status, eq(StatusCode::OK));
    expect_that!(first.outcome.items(), len(eq(2)));
    let link = first.head.typed::<Link>().into_valid().or_fail()?;
    for forged in ["AAAAAAAAAAAAAAAAAAAAAA", "zzz"] {
        let replayed =
            request_replacing(&tenant, &link, &RelationType::Next, "pageQuery", forged)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = tenant.send_unchecked(replayed).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let page = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&first.outcome.next_page().or_fail()?)
        .await?;
    expect_that!(page.status, eq(StatusCode::OK));
    expect_that!(page.outcome.items(), ids_in_order(&ids[2..]));
    for id in &ids {
        tenant.subscriptions().delete(id).send().await?;
    }
    Ok(())
}
