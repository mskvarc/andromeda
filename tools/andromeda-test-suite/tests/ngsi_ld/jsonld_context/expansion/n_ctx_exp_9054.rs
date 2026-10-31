//! A stored subscription jsonld context declaring a scoped context is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Create and Update Subscription and Context Source Registration Subscription whose `jsonldContext` names a
/// context defining gear with a scoped @context answer 400 `BadRequestData` and store nothing, while one naming a
/// plain context is created and keeps it
/// Clause: 4.4, 5.5.7, 5.8.1.4, 5.8.2.4, 5.11.2.4, 5.11.3.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9054_a_stored_subscription_jsonld_context_declaring_a_scoped_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_entity_scoped_id = namespace.subscription_id("entity-scoped")?;
    let device_type = namespace.entity_type("Device")?;
    let subscription_entity_plain_id = namespace.subscription_id("entity-plain")?;
    let subscription_csource_scoped_id = namespace.subscription_id("csource-scoped")?;
    let subscription_csource_plain_id = namespace.subscription_id("csource-plain")?;
    contexts
        .serve(
            "contexts/scoped.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "gear": {{
          "@id": "https://example.org/{namespace}/gear",
          "@context": {{
            "observedAt": "https://example.org/{namespace}/rewritten"
          }}
        }}
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "contexts/plain.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "gear": "https://example.org/{namespace}/gear"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let subscription_entity_scoped = Subscription::builder()
        .id(subscription_entity_scoped_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(contexts.url("contexts/scoped.jsonld")?)
        .build()?;
    let answer = broker.with_responses().subscriptions().create(&subscription_entity_scoped).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_entity_scoped_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_entity_plain = Subscription::builder()
        .id(subscription_entity_plain_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(contexts.url("contexts/plain.jsonld")?)
        .build()?;
    broker.subscriptions().create(&subscription_entity_plain).send().await?;
    let fragment = SubscriptionFragment::builder()
        .jsonld_context(contexts.url("contexts/scoped.jsonld")?)
        .build()?;
    let answer_3 = broker
        .with_responses()
        .subscriptions()
        .update(&subscription_entity_plain_id, &fragment)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_entity_plain_id)
        .send()
        .await?;
    expect_that!(subscription.jsonld_context(), some(eq(&contexts.url("contexts/plain.jsonld")?)));
    broker.subscriptions().delete(&subscription_entity_plain_id).send().await?;
    let subscription_csource_scoped = Subscription::registration_builder()
        .id(subscription_csource_scoped_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(contexts.url("contexts/scoped.jsonld")?)
        .build()?;
    let answer_4 = broker
        .with_responses()
        .registration_subscriptions()
        .create(&subscription_csource_scoped)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_csource_scoped_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let subscription_csource_plain = Subscription::registration_builder()
        .id(subscription_csource_plain_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(contexts.url("contexts/plain.jsonld")?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_csource_plain).send().await?;
    let fragment_2 = SubscriptionFragment::registration_builder()
        .jsonld_context(contexts.url("contexts/scoped.jsonld")?)
        .build()?;
    let answer_6 = broker
        .with_responses()
        .registration_subscriptions()
        .update(&subscription_csource_plain_id, &fragment_2)
        .send()
        .await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_csource_plain_id)
        .send()
        .await?;
    expect_that!(subscription_2.jsonld_context(), some(eq(&contexts.url("contexts/plain.jsonld")?)));
    broker.registration_subscriptions().delete(&subscription_csource_plain_id).send().await?;
    Ok(())
}
