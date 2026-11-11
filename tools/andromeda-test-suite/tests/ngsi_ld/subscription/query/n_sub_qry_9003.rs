//! Count reports the total beside a page or alone.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions over three subscriptions with limit 2 and count true answers two subscriptions and
/// NGSILD-Results-Count 3, and with limit 0 and count true answers an empty array and NGSILD-Results-Count 3
/// Clause: 4.12, 4.13, 6.3.13
/// Tenant: private (Query Subscriptions counts every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files change the total; clause 4.14 keeps each tenant's subscriptions its own)
#[gtest]
#[tokio::test]
async fn n_sub_qry_9003_count_reports_the_total_beside_a_page_or_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let tenant = broker::private_tenant(&front, &namespace).await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_type = namespace.entity_type("Building")?;
    for suffix in ["0000", "0001", "0002"] {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(suffix)?)
            .entities([building_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.subscriptions().create(&subscription).send().await?;
    }
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .query()
        .limit(2)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.total(), some(eq(3)));
    let count_only = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .query()
        .limit(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(count_only.items(), is_empty());
    expect_that!(count_only.total(), some(eq(3)));
    Ok(())
}
