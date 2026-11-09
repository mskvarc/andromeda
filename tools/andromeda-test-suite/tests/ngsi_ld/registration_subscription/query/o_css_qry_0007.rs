//! The last pointer leads to the final page.

use andromeda_test_suite::{broker, config::SuiteConfig, cursor_walk, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QuerySubscriptions},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, RegistrationSubscription, Subscription},
};

/// Pins: Query Context Source Registration Subscriptions with limit 2 and an empty cursor over five members offers a
/// last pointer leading to a page of two members holding the greatest identifier, with no next pointer
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (Query Context Source Registration Subscriptions pages through every one of the tenant, so in
/// the shared tenant the subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions
/// its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last
/// pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_css_qry_0007_the_last_pointer_leads_to_the_final_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let building_type = namespace.entity_type("Building")?;
    for suffix in ["0000", "0001", "0002", "0003", "0004"] {
        let subscription = Subscription::registration_builder()
            .id(namespace.subscription_id(suffix)?)
            .entities([building_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.registration_subscriptions().create(&subscription).send().await?;
    }
    let json_ld = tenant.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let query = QuerySubscriptions::builder().kind(RegistrationSubscription).limit(2).build();
    let first = cursor_walk::first_page(&json_ld, &query).await?.page;
    let final_page = json_ld.send(&first.last_page().or_fail()?).await?;
    expect_that!(final_page.next(), none());
    expect_that!(final_page.items(), len(eq(2)));
    expect_that!(final_page.items(), contains(has_id(&namespace.subscription_id("0004")?)));
    Ok(())
}
