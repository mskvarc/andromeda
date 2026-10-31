//! A scope query conjunction without parentheses is bad request data and changes nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSubscription, UpdateSubscription},
    },
    model::{ErrorType, NgsiLdMediaType, RawJson, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription with a `scopeQ` that joins scopes with ; outside parentheses is refused with
/// `BadRequestData` and keeps the stored `scopeQ`
/// Clause: 4.19, 5.8.2, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_upd_4001_a_scope_query_conjunction_without_parentheses_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription scopeQ), so `scopeQ` is written as text.
    let scope_q: RawJson = r#""/Madrid/Gardens/ParqueNorte""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("scopeQ", &scope_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid query language text at scopeQ), so `scopeQ` is written as text.
    let scope_q_2: RawJson = r#""/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC""#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment.render())?.set_member("scopeQ", &scope_q_2);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.scope_q().map(ToString::to_string), some(eq("/Madrid/Gardens/ParqueNorte")));
    Ok(())
}
