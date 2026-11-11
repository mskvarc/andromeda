//! Count reports the total beside a page or alone.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Context Source Registration Subscriptions over three subscriptions with limit 2 and count true answers
/// two subscriptions and NGSILD-Results-Count 3, and with limit 0 and count true answers an empty array and
/// NGSILD-Results-Count 3
/// Clause: 4.12, 4.13, 6.3.13
/// Tenant: private (Query Context Source Registration Subscriptions counts every one of the tenant, so in the shared
/// tenant the subscriptions of other files change the total; clause 4.14 keeps each tenant's subscriptions its own)
#[gtest]
#[tokio::test]
async fn n_css_qry_9004_count_reports_the_total_beside_a_page_or_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let building_type = namespace.entity_type("Building")?;
    for suffix in ["0000", "0001", "0002"] {
        let subscription = Subscription::registration_builder()
            .id(namespace.subscription_id(suffix)?)
            .entities([building_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(suffix)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.registration_subscriptions().create(&subscription).send().await?;
    }
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .query()
        .limit(2)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.total(), some(eq(3)));
    let count_only = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .query()
        .limit(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(count_only.items(), is_empty());
    expect_that!(count_only.total(), some(eq(3)));
    Ok(())
}
