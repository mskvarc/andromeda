//! Every link value is an rfc 8288 link value.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions as application/ld+json with limit 1 and offset 1 over three subscriptions answers a
/// Link field whose every comma-separated value opens with a URI-reference in angle brackets and carries a rel
/// Clause: 6.3.10, IETF RFC 8288 clause 3
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions its own)
#[gtest]
#[tokio::test]
async fn n_sub_qry_0017_every_link_value_is_an_rfc_8288_link_value() -> Result<()> {
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
    let headed = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .subscriptions()
        .query()
        .limit(1)
        .offset(1)
        .send()
        .await?;
    let link = headed.head.typed::<Link>().into_valid().or_fail()?;
    expect_that!(link.values(), not(is_empty()));
    for value in link.values() {
        expect_that!(value.rels(), not(is_empty()));
    }
    for suffix in ["0000", "0001", "0002"] {
        tenant.subscriptions().delete(&namespace.subscription_id(suffix)?).send().await?;
    }
    Ok(())
}
