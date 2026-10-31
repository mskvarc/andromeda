//! Subscriptions and registrations refuse an embedded context.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create and Update of a Subscription, a Context Source Registration Subscription and a Context Source
/// Registration with an @context inside an entity selector answer 400 `BadRequestData`, creating nothing and changing
/// nothing
/// Clause: 5.5.7, 5.8.1.4, 5.8.2.4, 5.11.2.4, 5.11.3.4, 5.9.2.4, 5.9.3.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9023_subscriptions_and_registrations_refuse_an_embedded_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let sink = NotificationSink::start().await?;
    let unreachable = config.unreachable_origin().as_str();
    let core = config.core_context();
    let device_type = namespace.entity_type("Device")?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"Device_{namespace}": "https://test.andromeda/{namespace}/Device"}}"#).parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    // The model refuses an @context inside an entity selector, so every refused document is sent as written.
    let refused_subscription = |kind: &str, id: &str, selector: &str, uri: &str| {
        format!(
            r#"{{
    "id": "{id}",
    "type": "{kind}",
    "entities": [ {selector} ],
    "notification": {{
        "endpoint": {{
            "uri": "{uri}",
            "accept": "application/json"
        }}
    }},
    "@context": [
        "{core}"
    ]
}}"#
        )
    };
    let refused_registration = |id: &str, selector: &str| {
        format!(
            r#"{{
    "id": "{id}",
    "type": "ContextSourceRegistration",
    "information": [ {{ "entities": [ {selector} ] }} ],
    "endpoint": "{unreachable}",
    "@context": [
        "{core}"
    ]
}}"#
        )
    };
    let refused_subscription_patch = |kind: &str, _id: &str, selector: &str, uri: &str| {
        format!(
            r#"{{
    "type": "{kind}",
    "entities": [ {selector} ],
    "notification": {{
        "endpoint": {{
            "uri": "{uri}",
            "accept": "application/json"
        }}
    }},
    "@context": [
        "{core}"
    ],
    "description": "patched"
}}"#
        )
    };
    let refused_registration_patch = |_id: &str, selector: &str| {
        format!(
            r#"{{
    "type": "ContextSourceRegistration",
    "information": [ {{ "entities": [ {selector} ] }} ],
    "endpoint": "{unreachable}",
    "@context": [
        "{core}"
    ],
    "description": "patched"
}}"#
        )
    };
    let inline_context_selector =
        format!(r#"{{"type": "Device_{namespace}", "@context": {{"Device_{namespace}": "https://example.org/{namespace}/Device"}}}}"#);
    let empty_context_selector = format!(r#"{{"type": "Device_{namespace}", "@context": {{}}}}"#);
    let refused_id = namespace.subscription_id("subscriptions-refused")?;
    let request = request_to(&broker, Method::POST, "subscriptions")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(refused_subscription(
            "Subscription",
            refused_id.as_str(),
            &inline_context_selector,
            sink.endpoint(1)?.as_str(),
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let absent = broker.with_responses().subscriptions().retrieve(&refused_id).send().await;
    expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    let kept_id = namespace.subscription_id("subscriptions-kept")?;
    let subscription = Subscription::builder()
        .id(kept_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(endpoint.clone()).build()?)
        .build()?;
    body_context.subscriptions().create(&subscription).send().await?;
    let request_2 = request_to(&broker, Method::PATCH, &format!("subscriptions/{kept_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(refused_subscription_patch(
            "Subscription",
            kept_id.as_str(),
            &empty_context_selector,
            sink.endpoint(1)?.as_str(),
        )));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let kept = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&kept_id)
        .send()
        .await?;
    expect_that!(kept.description(), none());
    let gone = broker.with_responses().subscriptions().delete(&kept_id).send().await;
    expect_that!(&gone, answered(StatusCode::NO_CONTENT));
    let csource_refused_id = namespace.subscription_id("csourceSubscriptions-refused")?;
    let request_3 = request_to(&broker, Method::POST, "csourceSubscriptions")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(refused_subscription(
            "Subscription",
            csource_refused_id.as_str(),
            &inline_context_selector,
            sink.endpoint(1)?.as_str(),
        )));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let csource_absent = broker.with_responses().registration_subscriptions().retrieve(&csource_refused_id).send().await;
    expect_that!(&csource_absent, answered(StatusCode::NOT_FOUND));
    let csource_kept_id = namespace.subscription_id("csourceSubscriptions-kept")?;
    let registration_subscription = Subscription::registration_builder()
        .id(csource_kept_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    body_context.registration_subscriptions().create(&registration_subscription).send().await?;
    let request_4 = request_to(&broker, Method::PATCH, &format!("csourceSubscriptions/{csource_kept_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(refused_subscription_patch(
            "Subscription",
            csource_kept_id.as_str(),
            &empty_context_selector,
            sink.endpoint(1)?.as_str(),
        )));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(
        &answer_4,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let csource_kept = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&csource_kept_id)
        .send()
        .await?;
    expect_that!(csource_kept.description(), none());
    let csource_gone = broker.with_responses().registration_subscriptions().delete(&csource_kept_id).send().await;
    expect_that!(&csource_gone, answered(StatusCode::NO_CONTENT));
    let registration_refused_id = namespace.registration_id("csourceRegistrations-refused")?;
    let request_5 = request_to(&broker, Method::POST, "csourceRegistrations")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(refused_registration(registration_refused_id.as_str(), &inline_context_selector)));
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let registration_absent = broker.with_responses().registrations().retrieve(&registration_refused_id).send().await;
    expect_that!(&registration_absent, answered(StatusCode::NOT_FOUND));
    let registration_kept_id = namespace.registration_id("csourceRegistrations-kept")?;
    let registration = CSourceRegistration::builder(unreachable)
        .id(registration_kept_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    body_context.registrations().register(&registration).send().await?;
    let request_6 = request_to(&broker, Method::PATCH, &format!("csourceRegistrations/{registration_kept_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(refused_registration_patch(registration_kept_id.as_str(), &empty_context_selector)));
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(
        &answer_6,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let registration_kept = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_kept_id)
        .send()
        .await?;
    expect_that!(registration_kept.description(), none());
    let registration_gone = broker.with_responses().registrations().delete(&registration_kept_id).send().await;
    expect_that!(&registration_gone, answered(StatusCode::NO_CONTENT));
    Ok(())
}
