//! Listed attributes with sys attrs keep the system times and leave scope out.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SystemAttributeReporting},
};

/// Pins: A notification of an append on a subscription listing the attribute name with `sysAttrs` true carries name
/// with its new value, `createdAt` and `modifiedAt`, and not scope, the unlisted Attributes or `deletedAt`
/// Clause: 5.8.6, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0016_listed_attributes_with_sys_attrs_keep_the_system_times_and_leave_scope_out() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let city = Entity::builder().id(city_id.clone()).entity_type(city_type).build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .scope("/Paris/Tourism")
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .relationship("locatedAt", city_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .attributes(["name"])
                .system_attributes(SystemAttributeReporting::Include)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("name", "Tour Eiffel").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    let item = notification.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(system_time::of_entity(item, SystemMember::ModifiedAt).ok(), some(anything()));
    expect_that!(item.scope.as_ref(), none());
    expect_that!(system_time::of_entity(item, SystemMember::DeletedAt).ok(), none());
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_attribute("name", any_attribute().value("Tour Eiffel")),
                lacks_attribute("subCategory"),
                lacks_attribute("locatedAt")
            )
        )
    );
    Ok(())
}
