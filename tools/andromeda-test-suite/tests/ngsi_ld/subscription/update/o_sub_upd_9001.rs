//! An update adding attributes beside pick or omit is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, subscription_document::with_notification_members};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::UpdateSubscription,
    model::{ErrorType, NgsiLdMediaType},
    query::projection::{self, EntityMember, Projection},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription with a notification fragment holding attributes beside pick or beside omit answers 400
/// `BadRequestData`
/// Clause: 4.21, 5.2.14, 5.8.2.4
/// Recorded: no clause forbids combining pick or omit with the deprecated attrs, which Tables 6.4.3.2-1, 6.5.3.1-1
/// and 5.2.14.1-1 make a synonym for pick (attributes in a notification); Andromeda refuses attrs beside pick or
/// omit with `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_sub_upd_9001_an_update_adding_attributes_beside_pick_or_omit_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("update")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let id_only = || -> Result<Projection> { Ok(projection::member(EntityMember::Id).build()?) };
    let endpoint = || -> Result<Endpoint> { Ok(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?) };
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(endpoint()?).pick(id_only()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    for (pick, omit) in [(Some(id_only()?), None), (None, Some(id_only()?))] {
        let fragment = SubscriptionFragment::builder()
            .notification(NotificationParams::builder(endpoint()?).maybe_pick(pick).maybe_omit(omit).build()?)
            .build()?;
        // `id` is a reserved member, so the model builds no `attributes` naming it and that member is added to the written notification.
        let document = with_notification_members(&fragment.render(), &[("attributes", r#"["id"]"#)])?;
        let request = broker
            .unchecked(&UpdateSubscription {
                id: &subscription_id,
                fragment: &fragment,
            })?
            .document(&document);
        let refused = broker.send_unchecked(request).await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
