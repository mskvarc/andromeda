//! The pointers carry the requested json ld media type.

use andromeda_test_suite::{broker, config::SuiteConfig, link_matcher::link_value, namespace::Namespace, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions as application/ld+json with limit 1 and offset 1 over three subscriptions answers
/// pointers of type application/ld+json
/// Clause: 6.3.10
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions its own)
#[gtest]
#[tokio::test]
async fn n_sub_qry_0015_the_pointers_carry_the_requested_json_ld_media_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0001_id = namespace.subscription_id("0001")?;
    let subscription_0002_id = namespace.subscription_id("0002")?;
    let subscription_0000 = Subscription::builder()
        .id(subscription_0000_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0000).send().await?;
    let subscription_0001 = Subscription::builder()
        .id(subscription_0001_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0001).send().await?;
    let subscription_0002 = Subscription::builder()
        .id(subscription_0002_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0002).send().await?;
    let headed = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .subscriptions()
        .query()
        .limit(1)
        .offset(1)
        .send()
        .await?;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(
        headed.head.typed::<Link>().valid(),
        some(link_value(RelationType::Next).with_media_type("application/ld+json"))
    );
    expect_that!(
        headed.head.typed::<Link>().valid(),
        some(link_value(RelationType::Prev).with_media_type("application/ld+json"))
    );
    Ok(())
}
