//! Notification attributes naming an entity member is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
    subscription_document::create_request,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, InlineContext, Input, NgsiLdMediaType},
    query::projection::Projection,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Create Subscription whose `notification.attributes` names `id`, `type` or `scope` beside the Attribute
/// speed, or names a term the request `@context` defines as the Scope, answers 400 `BadRequestData` naming that
/// member, and Retrieve Subscription answers 404; `notification.attributes` naming speed alone, and `pick` naming
/// `id`, `type` and `scope`, are created
/// Clause: Table 5.2.14.1-1, 5.5.4, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9036_notification_attributes_naming_an_entity_member_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let endpoint = || -> Result<Endpoint> { Ok(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?) };
    let subscription = |id: &SubscriptionId, notification: NotificationParams<Input>| -> Result<Subscription<Input>> {
        Ok(Subscription::builder()
            .id(id.clone())
            .entities([vehicle_type.as_str()])
            .notification(notification)
            .build()?)
    };
    let reading = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    // The model builds no `attributes` naming an Entity member, so that member is added to the written notification.
    for member in ["id", "type", "scope"] {
        let id = namespace.subscription_id(member)?;
        let written = subscription(&id, NotificationParams::builder(endpoint()?).build()?)?;
        let attributes = format!(r#"["speed", "{member}"]"#);
        let refused = broker.send_unchecked(create_request(&broker, &written, &[("attributes", &attributes)])?).await;
        expect_that!(
            &refused,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
                .with_detail(&format!("'{member}' is an Entity member and is not allowed in notification.attributes"))
        );
        let unstored = reading.subscriptions().retrieve(&id).send().await;
        expect_that!(&unstored, answered(StatusCode::NOT_FOUND));
    }
    let aliasing = broker
        .with_context(Context::Multiple(vec![ContextEntry::Inline(
            r#"{"sc": "https://uri.etsi.org/ngsi-ld/scope"}"#.parse::<InlineContext>()?,
        )]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let alias_id = namespace.subscription_id("scope-alias")?;
    let alias = subscription(&alias_id, NotificationParams::builder(endpoint()?).attributes(["speed", "sc"]).build()?)?;
    let refused_alias = aliasing.with_responses().subscriptions().create(&alias).send().await;
    expect_that!(
        &refused_alias,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("'sc' is an Entity member and is not allowed in notification.attributes")
    );
    let unstored_alias = reading.subscriptions().retrieve(&alias_id).send().await;
    expect_that!(&unstored_alias, answered(StatusCode::NOT_FOUND));
    let attributes_id = namespace.subscription_id("attributes")?;
    let attributes = subscription(&attributes_id, NotificationParams::builder(endpoint()?).attributes(["speed"]).build()?)?;
    broker.subscriptions().create(&attributes).send().await?;
    broker.subscriptions().delete(&attributes_id).send().await?;
    let pick_id = namespace.subscription_id("pick")?;
    let picked = subscription(
        &pick_id,
        NotificationParams::builder(endpoint()?).pick("id,type,scope".parse::<Projection>()?).build()?,
    )?;
    broker.subscriptions().create(&picked).send().await?;
    broker.subscriptions().delete(&pick_id).send().await?;
    Ok(())
}
