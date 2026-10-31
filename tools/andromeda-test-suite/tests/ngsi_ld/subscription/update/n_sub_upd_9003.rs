//! Ngsild conformance reads back and is replaced only when named.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSubscription, UpdateSubscription},
    },
    model::{ConformanceVersion, NgsiLdMediaType, RawJson},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: A Subscription created with `ngsildConformance` 1.4 reads it back, an update naming 1.0 replaces it, and a
/// later update naming only description leaves 1.0
/// Clause: Table 5.2.12-1, 5.8.2, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_upd_9003_ngsild_conformance_reads_back_and_is_replaced_only_when_named() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_conformance_id = namespace.subscription_id("conformance")?;
    let vehicle_conformance_id = namespace.urn("Vehicle", "conformance")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_conformance = Subscription::builder()
        .id(subscription_conformance_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_conformance_id)])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance: RawJson = r#""1.4""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_conformance.render())?.set_member("ngsildConformance", &ngsild_conformance);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_conformance,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_conformance_id)
        .send()
        .await?;
    expect_that!(subscription.conformance(), some(eq("1.4".parse::<ConformanceVersion>()?)));
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (a subscription ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance_2: RawJson = r#""1.0""#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment.render())?.set_member("ngsildConformance", &ngsild_conformance_2);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_conformance_id,
            fragment: &fragment,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_conformance_id)
        .send()
        .await?;
    expect_that!(subscription_2.conformance(), some(eq("1.0".parse::<ConformanceVersion>()?)));
    let fragment_2 = SubscriptionFragment::builder().description("unrelated").build()?;
    broker.subscriptions().update(&subscription_conformance_id, &fragment_2).send().await?;
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_conformance_id)
        .send()
        .await?;
    expect_that!(subscription_3.conformance(), some(eq("1.0".parse::<ConformanceVersion>()?)));
    broker.subscriptions().delete(&subscription_conformance_id).send().await?;
    Ok(())
}
