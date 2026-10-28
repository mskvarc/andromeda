//! ETSI `028_07`: a create-subscription request whose `jsonldContext` is not a retrievable context is refused, with 400
//! `BadRequestData` when the value is not an IRI and 504 `LdContextNotAvailable` when it names an unreachable document
//! (clauses 5.8.1 and 6.3.2).

use crate::{
    suite_keywords::{ld_json_body_client, refused},
    suite_payloads::building_subscription,
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::problem,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, Representation},
    subscription::{Endpoint, SubscriptionId},
};

/// Pins: Check that one cannot create a subscription with invalid or unavailable jsonldContext
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_07_01_subscription_with_invalid_jsonld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::JsonLd).build()?;
    let subscription = building_subscription(id, endpoint, Representation::Normalized)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_text("jsonldContext", "unknownContext");
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    let request = client.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    refused(&client, request, ErrorType::BadRequestData).await
}

/// Pins: Check that one cannot create a subscription with invalid or unavailable jsonldContext
/// Clause: 5.8.1
/// Corrected: fix(ldcontextnotavailable): ldcontextnotavailable is 504 not 503 (table 6.3.2-1)
#[gtest]
#[tokio::test]
async fn e028_07_02_subscription_with_unavailable_jsonld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::JsonLd).build()?;
    let subscription = building_subscription(id, endpoint, Representation::Normalized)?;
    let unavailable = SuiteConfig::get()?
        .unreachable_origin()
        .join(&format!("contexts/{namespace}/ngsi-ld-core-context-non-existing.jsonld"))?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_text("jsonldContext", unavailable.as_str());
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    let request = client.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable).with_title())
}
