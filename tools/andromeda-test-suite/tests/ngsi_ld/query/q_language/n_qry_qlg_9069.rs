//! A q or csf outside the grammar is bad request data in every carrier.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    raw_call::target,
    wire::request_to,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Sends Query Entities by GET with the query text `q`, as written, and checks the refusal.
async fn expect_get_refused(broker: &BrokerClient, car_type: &str, q: &str) -> Result<()> {
    // The parameter model refuses these queries, so the request is sent as written.
    let request = request_to(broker, Method::GET, &target("entities", &[("type", car_type), ("q", q)]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}

/// Sends Query Entities by POST with `member` set to `text` in the query document, as written, and checks the refusal.
async fn expect_post_refused(broker: &BrokerClient, car_type: &str, member: &str, text: &str) -> Result<()> {
    // The query document carries a member outside the grammar, so it is sent as written.
    let body = format!(r#"{{"type": "Query", "entities": [{{"type": "{car_type}"}}], "{member}": "{text}"}}"#);
    let request = json_request_to(broker, Method::POST, "entityOperations/query", &body)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}

/// Pins: a q or csf outside the clause 4.9 grammar (an operator with no value, an unbalanced parenthesis, a name
/// outside the name grammar) is `BadRequestData` whether a GET, a POST query or a Subscription carries it, and no
/// Subscription is created
/// Clause: 4.9, 4.6.2, 5.7.2.4, 5.8.1.4, 6.4.3.2, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9069_a_q_or_csf_outside_the_grammar_is_bad_request_data_in_every_carrier() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let car_type = namespace.entity_type("Car")?;
    let subscription_q_id = namespace.subscription_id("q")?;
    let subscription_csf_id = namespace.subscription_id("csf")?;
    expect_get_refused(&broker, car_type.as_str(), "speed==").await?;
    expect_get_refused(&broker, car_type.as_str(), "speed-limit>1").await?;
    expect_post_refused(&broker, car_type.as_str(), "q", "(speed>1").await?;
    expect_post_refused(&broker, car_type.as_str(), "q", "speed-limit>1").await?;
    expect_post_refused(&broker, car_type.as_str(), "csf", "tier==").await?;
    let subscription_q = Subscription::builder()
        .id(subscription_q_id.clone())
        .entities([car_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("q")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The grammar refuses the text, so `q` is written as text.
    let document_q = UncheckedDocument::try_from(subscription_q.render())?.set_text("q", "speed>1)");
    let request_q = broker.unchecked(&CreateSubscription { subscription: &subscription_q })?.document(&document_q);
    let answer_q = broker.send_unchecked(request_q).await;
    expect_that!(&answer_q, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_csf = Subscription::builder()
        .id(subscription_csf_id.clone())
        .entities([car_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("csf")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The grammar refuses the text, so `csf` is written as text.
    let document_csf = UncheckedDocument::try_from(subscription_csf.render())?.set_text("csf", "tier==");
    let request_csf = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_csf,
        })?
        .document(&document_csf);
    let answer_csf = broker.send_unchecked(request_csf).await;
    expect_that!(&answer_csf, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_q_read = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_q_id)
        .send()
        .await;
    expect_that!(&answer_q_read, answered(StatusCode::NOT_FOUND));
    let answer_csf_read = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_csf_id)
        .send()
        .await;
    expect_that!(&answer_csf_read, answered(StatusCode::NOT_FOUND));
    Ok(())
}
