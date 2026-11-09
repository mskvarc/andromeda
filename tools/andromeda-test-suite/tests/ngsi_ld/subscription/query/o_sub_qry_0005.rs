//! The last pointer leads to the final page.

use andromeda_test_suite::{broker, config::SuiteConfig, cursor_walk, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QuerySubscriptions},
    model::NgsiLdMediaType,
    subscription::{Endpoint, EntitySubscription, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions with limit 2 and an empty cursor over five members offers a last pointer leading to a
/// page of two members holding the greatest identifier, with no next pointer
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last
/// pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_sub_qry_0005_the_last_pointer_leads_to_the_final_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let tenant = broker::private_tenant(&front, &namespace).await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_type = namespace.entity_type("Building")?;
    for suffix in ["0000", "0001", "0002", "0003", "0004"] {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(suffix)?)
            .entities([building_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.subscriptions().create(&subscription).send().await?;
    }
    let json = tenant.with_accept(Accept::new(NgsiLdMediaType::Json));
    let query = QuerySubscriptions::builder().kind(EntitySubscription).limit(2).build();
    let first = cursor_walk::first_page(&json, &query).await?.page;
    let final_page = json.send(&first.last_page().or_fail()?).await?;
    expect_that!(final_page.items(), len(eq(2)));
    expect_that!(final_page.items(), contains(has_id(&namespace.subscription_id("0004")?)));
    expect_that!(final_page.next(), none());
    for suffix in ["0000", "0001", "0002", "0003", "0004"] {
        tenant.subscriptions().delete(&namespace.subscription_id(suffix)?).send().await?;
    }
    Ok(())
}
