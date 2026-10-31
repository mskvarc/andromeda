//! The last page asked by offset offers only prev.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Context Source Registration Subscriptions with limit 1 and offset 2 over three subscriptions answers
/// a prev pointer and no next pointer, and no pointer naming a page parameter
/// Clause: 5.5.9.1, 6.3.10
/// Tenant: private (Query Context Source Registration Subscriptions pages through every one of the tenant, so in
/// the shared tenant the subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions
/// its own)
#[gtest]
#[tokio::test]
async fn n_css_qry_9003_the_last_page_asked_by_offset_offers_only_prev() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0001_id = namespace.subscription_id("0001")?;
    let subscription_0002_id = namespace.subscription_id("0002")?;
    // A registration created and deleted again brings the tenant into existence and leaves nothing in it.
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let subscription_0000 = Subscription::registration_builder()
        .id(subscription_0000_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("0000")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0000).send().await?;
    let subscription_0001 = Subscription::registration_builder()
        .id(subscription_0001_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("0001")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0001).send().await?;
    let subscription_0002 = Subscription::registration_builder()
        .id(subscription_0002_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("0002")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0002).send().await?;
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registration_subscriptions()
        .query()
        .limit(1)
        .offset(2)
        .send()
        .await?;
    expect_that!(links(&page), each(not(carries_parameter("page"))));
    expect_that!(page.prev(), some(anything()));
    expect_that!(page.next(), none());
    Ok(())
}
