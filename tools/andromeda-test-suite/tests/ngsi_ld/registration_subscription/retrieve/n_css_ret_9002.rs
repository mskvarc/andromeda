//! Ngsild conformance reads back on a context source registration subscription.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ConformanceVersion, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription created with `ngsildConformance` 1.4 reads it back
/// Clause: Table 5.2.12-1, 5.11.2, 5.11.3
#[gtest]
#[tokio::test]
async fn n_css_ret_9002_ngsild_conformance_reads_back_on_a_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_csource_id = namespace.subscription_id("csource")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_csource = Subscription::registration_builder()
        .id(subscription_csource_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance: RawJson = r#""1.4""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_csource.render())?.set_member("ngsildConformance", &ngsild_conformance);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_csource,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_csource_id)
        .send()
        .await?;
    expect_that!(subscription.conformance(), some(eq("1.4".parse::<ConformanceVersion>()?)));
    broker.registration_subscriptions().delete(&subscription_csource_id).send().await?;
    Ok(())
}
