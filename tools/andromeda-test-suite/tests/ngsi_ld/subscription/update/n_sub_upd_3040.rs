//! A type selection outside the grammar in an updated entity selector is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, nth::nth, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    query::{EntitySelector, SelectorType, builders::SelectorTypeBuilder, type_selection},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription refuses an entities selector whose type is a type selection outside the 4.17 grammar
/// with `BadRequestData` and keeps the stored selector, and accepts the parenthesised conjunction
/// Clause: 4.17, 5.2.33, 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_3040_a_type_selection_outside_the_grammar_in_an_updated_entity_selector_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let home_type = namespace.entity_type("Home")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([home_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    // Table 5.2.33-1: type is "A valid type selection string as per clause 4.17"
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid entity selector at entities.0.type), so `entities` is written as text.
    let entities: RawJson = format!(r#"[{{"type": "Home_{namespace};Vehicle_{namespace}"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("entities", &entities);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let fragment_2 = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid entity selector at entities.0.type), so `entities` is written as text.
    let entities_2: RawJson = format!(r#"[{{"type": "(Home_{namespace}|Vehicle_{namespace})"}}]"#).parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_2.render())?.set_member("entities", &entities_2);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment_2,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        subscription_2.entities().iter().map(EntitySelector::selector_type).collect::<Vec<_>>(),
        nth(0, eq(&&SelectorTypeBuilder::from(&home_type).build()?))
    );
    // control: the parenthesised conjunction is accepted
    let fragment_3 = SubscriptionFragment::builder()
        .entities([EntitySelector::new(SelectorType::Selection(
            type_selection::all_of([&home_type, &namespace.entity_type("Vehicle")?]).build()?,
        ))])
        .build()?;
    broker.subscriptions().update(&subscription_id, &fragment_3).send().await?;
    Ok(())
}
