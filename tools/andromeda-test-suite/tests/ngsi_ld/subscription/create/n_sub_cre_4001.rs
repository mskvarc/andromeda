//! A scope query conjunction without parentheses is bad request data and stores nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with a `scopeQ` that joins scopes with ; outside parentheses is refused with
/// `BadRequestData` and stores nothing, and the parenthesised conjunction is accepted
/// Clause: 4.19, 5.8.1.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_4001_a_scope_query_conjunction_without_parentheses_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (invalid query language text at scopeQ), so `scopeQ` is written as text.
    let scope_q: RawJson = r#""/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("scopeQ", &scope_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription scopeQ), so `scopeQ` is written as text.
    let scope_q_2: RawJson = r#""(/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC)""#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_2.render())?.set_member("scopeQ", &scope_q_2);
    let request_2 = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?.document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, answered(StatusCode::CREATED));
    Ok(())
}
