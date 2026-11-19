//! An update naming an entity member in notification attributes is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, subscription_document::with_notification_members};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::UpdateSubscription},
    model::{Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription of a Subscription notifying speed with a fragment whose `notification.attributes` names
/// `id`, `type` or `scope` beside speed, or names a term the request `@context` defines as the Scope, answers 400
/// `BadRequestData` naming that member, and Retrieve Subscription still answers `notification.attributes` as speed
/// Clause: Table 5.2.14.1-1, 5.5.4, 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_9016_an_update_naming_an_entity_member_in_notification_attributes_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("update")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let endpoint = || -> Result<Endpoint> { Ok(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?) };
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(endpoint()?).attributes(["speed"]).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder()
        .notification(NotificationParams::builder(endpoint()?).build()?)
        .build()?;
    // The model builds no `attributes` naming an Entity member, so that member is added to the written notification.
    for member in ["id", "type", "scope"] {
        let attributes = format!(r#"["speed", "{member}"]"#);
        let document = with_notification_members(&fragment.render(), &[("attributes", &attributes)])?;
        let request = broker
            .unchecked(&UpdateSubscription {
                id: &subscription_id,
                fragment: &fragment,
            })?
            .document(&document);
        let refused = broker.send_unchecked(request).await;
        expect_that!(
            &refused,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
                .with_detail(&format!("'{member}' is an Entity member and is not allowed in notification.attributes"))
        );
    }
    let aliasing = broker
        .with_context(Context::Multiple(vec![ContextEntry::Inline(
            r#"{"sc": "https://uri.etsi.org/ngsi-ld/scope"}"#.parse::<InlineContext>()?,
        )]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let alias = SubscriptionFragment::builder()
        .notification(NotificationParams::builder(endpoint()?).attributes(["speed", "sc"]).build()?)
        .build()?;
    let refused_alias = aliasing.with_responses().subscriptions().update(&subscription_id, &alias).send().await;
    expect_that!(
        &refused_alias,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("'sc' is an Entity member and is not allowed in notification.attributes")
    );
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        stored.notification().attributes().iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq("speed")]
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
