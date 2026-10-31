//! Attributes beside pick or omit in a notification is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, subscription_document::create_request};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{ErrorType, Input, NgsiLdMediaType},
    query::projection::{self, EntityMember, Projection},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Create Subscription with a notification holding attributes beside pick or beside omit answers 400
/// `BadRequestData`, and with pick, omit or attributes alone answers 201
/// Clause: 4.21, 5.2.14, 5.8.1.4
/// Recorded: no clause forbids combining pick or omit with the deprecated attrs, which Tables 6.4.3.2-1, 6.5.3.1-1
/// and 5.2.14.1-1 make a synonym for pick (attributes in a notification); Andromeda refuses attrs beside pick or
/// omit with `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_sub_cre_9011_attributes_beside_pick_or_omit_in_a_notification_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let id_only = || -> Result<Projection> { Ok(projection::member(EntityMember::Id).build()?) };
    let subscription = |id: &SubscriptionId, pick: Option<Projection>, omit: Option<Projection>| -> Result<Subscription<Input>> {
        let notification = NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?)
            .maybe_pick(pick)
            .maybe_omit(omit)
            .build()?;
        Ok(Subscription::builder()
            .id(id.clone())
            .entities([vehicle_type.as_str()])
            .notification(notification)
            .build()?)
    };
    // The model builds no `attributes` beside pick or omit, so that member is added to the written body; it names an
    // Attribute, as Table 5.2.14-1 allows `attributes` no `id`, `type` or `scope`.
    for (suffix, pick, omit) in [("refused-0", Some(id_only()?), None), ("refused-1", None, Some(id_only()?))] {
        let id = namespace.subscription_id(suffix)?;
        let refused = broker
            .send_unchecked(create_request(&broker, &subscription(&id, pick, omit)?, &[("attributes", r#"["speed"]"#)])?)
            .await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    for (suffix, pick, omit) in [("admitted-pick", Some(id_only()?), None), ("admitted-omit", None, Some(id_only()?))] {
        let id = namespace.subscription_id(suffix)?;
        broker.subscriptions().create(&subscription(&id, pick, omit)?).send().await?;
        broker.subscriptions().delete(&id).send().await?;
    }
    let attributes_id = namespace.subscription_id("admitted-attributes")?;
    let notification = NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?)
        .attributes(["speed"])
        .build()?;
    let attributes = Subscription::builder()
        .id(attributes_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(notification)
        .build()?;
    broker.subscriptions().create(&attributes).send().await?;
    broker.subscriptions().delete(&attributes_id).send().await?;
    Ok(())
}
