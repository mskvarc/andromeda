//! An update leaving no entities or watched attributes is bad request data and changes nothing.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, nth::nth, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Context Source Registration Subscription with entities written as the NGSI-LD Null on a
/// subscription without `watchedAttributes` or `localOnly` answers 400 `BadRequestData` and keeps the stored
/// entities
/// Clause: 5.2.12, 5.5.8, 5.11.3.4
/// Recorded: clause 5.11.3.4 checks the restrictions of clause 5.2.12 against the Subscription Fragment, which is
/// valid on its own, and does not say what happens when merging it by clause 5.5.8 would leave a subscription that
/// clause 5.2.12 forbids ("At least one of (a) entities or (b) `watchedAttributes` shall be present, unless the
/// member `localOnly` is set to true"); Andromeda validates the merged subscription and refuses the update
#[gtest]
#[tokio::test]
async fn o_css_upd_9004_an_update_leaving_no_entities_or_watched_attributes_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::registration_builder().build()?;
    // The model refuses it (expected a JSON array at entities), so `entities` is written as text.
    let entities: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("entities", &entities);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.entities(), len(eq(1)));
    expect_that!(
        subscription_2.entities().iter().map(EntitySelector::selector_type).collect::<Vec<_>>(),
        nth(0, eq(&&SelectorTypeBuilder::from(&building_type).build()?))
    );
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
