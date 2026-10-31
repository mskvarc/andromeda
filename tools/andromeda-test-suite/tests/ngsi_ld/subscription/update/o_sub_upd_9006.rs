//! An update leaving no entities or watched attributes is bad request data and changes nothing.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    nth::nth,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{AttributeName, ErrorType, NgsiLdMediaType, RawJson},
    query::{EntitySelector, Locality, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription whose merged result names neither entities nor `watchedAttributes` and is not
/// `localOnly` answers 400 `BadRequestData` and leaves the subscription unchanged: entities written as the NGSI-LD
/// Null on a subscription without `watchedAttributes`, entities deleted on a periodic one, `localOnly` deleted or
/// false on a local one without a selector; entities deleted beside stored `watchedAttributes` answers 204 and
/// keeps `watchedAttributes`
/// Clause: 5.2.12, 5.5.8, 5.8.2.4
/// Recorded: clause 5.8.2.4 checks the restrictions of clause 5.2.12 against the Subscription Fragment, which is
/// valid on its own, and does not say what happens when merging it by clause 5.5.8 would leave a subscription that
/// clause 5.2.12 forbids ("At least one of (a) entities or (b) `watchedAttributes` shall be present, unless the
/// member `localOnly` is set to true"); Andromeda validates the merged subscription and refuses the update
#[gtest]
#[tokio::test]
async fn o_sub_upd_9006_an_update_leaving_no_entities_or_watched_attributes_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_entities_id = namespace.subscription_id("entities")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_periodic_id = namespace.subscription_id("periodic")?;
    let subscription_local_id = namespace.subscription_id("local")?;
    let subscription_watched_id = namespace.subscription_id("watched")?;
    let subscription_entities = Subscription::builder()
        .id(subscription_entities_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_entities).send().await?;
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (expected a JSON array at entities), so `entities` is written as text.
    let entities: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("entities", &entities);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_entities_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_entities_id)
        .send()
        .await?;
    expect_that!(subscription.entities(), len(eq(1)));
    expect_that!(
        subscription.entities().iter().map(EntitySelector::selector_type).collect::<Vec<_>>(),
        nth(0, eq(&&SelectorTypeBuilder::from(&building_type).build()?))
    );
    let subscription_periodic = Subscription::builder()
        .id(subscription_periodic_id.clone())
        .entities([building_type.as_str()])
        .time_interval(3600)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_periodic).send().await?;
    let fragment_2 = SubscriptionFragment::builder().build()?;
    // The model refuses it (expected a JSON array at entities), so `entities` is written as text.
    let entities_2: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_2.render())?.set_member("entities", &entities_2);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_periodic_id,
            fragment: &fragment_2,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_periodic_id)
        .send()
        .await?;
    expect_that!(subscription_2.entities(), len(eq(1)));
    expect_that!(subscription_2.time_interval().and_then(|number| number.as_ref().as_i64()), some(eq(3600)));
    let subscription_local = Subscription::builder()
        .id(subscription_local_id.clone())
        .locality(Locality::Local)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_local).send().await?;
    let fragment_3 = SubscriptionFragment::builder().build()?;
    // The model refuses it (`localOnly` must be a boolean at localOnly), so `localOnly` is written as text.
    let local_only: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document_3 = UncheckedDocument::try_from(fragment_3.render())?.set_member("localOnly", &local_only);
    let request_3 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_local_id,
            fragment: &fragment_3,
        })?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_4 = SubscriptionFragment::builder().locality(Locality::Distributed).build()?;
    let answer_4 = broker.with_responses().subscriptions().update(&subscription_local_id, &fragment_4).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_local_id)
        .send()
        .await?;
    expect_that!(subscription_3.locality(), eq(Locality::Local));
    expect_that!(subscription_3.entities(), is_empty());
    expect_that!(subscription_3.watched_attributes(), is_empty());
    let subscription_watched = Subscription::builder()
        .id(subscription_watched_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["name"])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_watched).send().await?;
    let fragment_5 = SubscriptionFragment::builder().build()?;
    // The model refuses it (expected a JSON array at entities), so `entities` is written as text.
    let entities_3: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document_4 = UncheckedDocument::try_from(fragment_5.render())?.set_member("entities", &entities_3);
    let request_4 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_watched_id,
            fragment: &fragment_5,
        })?
        .document(&document_4);
    let answer_5 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_5, answered(StatusCode::NO_CONTENT));
    let subscription_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_watched_id)
        .send()
        .await?;
    expect_that!(subscription_4.entities(), is_empty());
    expect_that!(subscription_4.watched_attributes(), len(eq(1)));
    expect_that!(subscription_4.watched_attributes(), nth(0, eq(&"name".parse::<AttributeName>()?)));
    broker.subscriptions().delete(&subscription_entities_id).send().await?;
    broker.subscriptions().delete(&subscription_periodic_id).send().await?;
    broker.subscriptions().delete(&subscription_local_id).send().await?;
    broker.subscriptions().delete(&subscription_watched_id).send().await?;
    Ok(())
}
