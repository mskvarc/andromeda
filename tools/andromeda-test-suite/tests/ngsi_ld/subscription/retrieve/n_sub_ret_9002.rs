//! A notification endpoint reads back as written and unnotified counters are absent.

use andromeda_test_suite::{body_json::json_equals, broker, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{NgsiLdMediaType, RawJson},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription whose notification endpoint uri is an authority-only URI, and one whose uri has a path,
/// read the uri back as written and carry no status, `timesSent` or `timesFailed`
/// Clause: 5.2.2, Table 5.2.14.2-1, Table 5.2.15-1, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_ret_9002_a_notification_endpoint_reads_back_as_written_and_unnotified_counters_are_absent() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let selector = EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse()?);
    for (suffix, uri) in [("spelling-0", unreachable.clone()), ("spelling-1", format!("{unreachable}/notify"))] {
        let id = namespace.subscription_id(suffix)?;
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([selector.clone()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        // The endpoint model gives an authority-only URI its path, and the subject is the spelling, so the notification is written as given.
        let notification: RawJson = format!(r#"{{"endpoint": {{"uri": "{uri}", "accept": "application/json"}}}}"#).parse()?;
        let document = UncheckedDocument::try_from(subscription.render())?.set_member("notification", &notification);
        let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
        let created = broker.send_unchecked(request).await;
        expect_that!(&created, answered(StatusCode::CREATED));
        // The typed endpoint holds a URL, which has the path an authority-only URI lacks, so the spelling is read from the body.
        let stored = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&id)
            .send()
            .await?;
        expect_that!(&stored.body, json_equals("$.notification.endpoint.uri", uri));
        let output = stored.outcome.notification().output();
        expect_that!(output.status, none());
        expect_that!(output.times_sent, none());
        expect_that!(output.times_failed, none());
        broker.subscriptions().delete(&id).send().await?;
    }
    Ok(())
}
