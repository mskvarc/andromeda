//! An offset at or past the end answers an empty array with only prev.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions with limit 2 over two subscriptions and an offset of 2 or 3 answers 200 with an empty
/// array and a prev pointer alone
/// Clause: 5.5.9.2, 6.3.10, Table 6.3.10-2
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files change the size of the result set; clause 4.14 keeps each tenant's subscriptions its
/// own)
/// Recorded: Table 6.3.10-2 makes the too-large offset a "should" and its rule sets the offset to the size of the result
/// set, which selects nothing; Andromeda answers 200 with an empty array
#[gtest]
#[tokio::test]
async fn o_sub_qry_9002_an_offset_at_or_past_the_end_answers_an_empty_array_with_only_prev() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let tenant = broker::private_tenant(&front, &namespace).await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_type = namespace.entity_type("Building")?;
    for suffix in ["0000", "0001"] {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(suffix)?)
            .entities([building_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.subscriptions().create(&subscription).send().await?;
    }
    for offset in [2, 3] {
        let page = tenant
            .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
            .subscriptions()
            .query()
            .limit(2)
            .offset(offset)
            .send()
            .await?;
        expect_that!(page.items(), is_empty());
        expect_that!(page.prev(), some(anything()));
        expect_that!(page.next(), none());
        expect_that!(page.first(), none());
        expect_that!(page.last(), none());
    }
    Ok(())
}
