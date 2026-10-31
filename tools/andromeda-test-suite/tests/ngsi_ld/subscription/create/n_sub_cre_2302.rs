//! An expires at with a comma fraction is accepted.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{DateTime, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription accepts an `expiresAt` whose seconds fraction is separated by a comma
/// Clause: 4.6.3, 5.8.1
#[gtest]
#[tokio::test]
async fn n_sub_cre_2302_an_expires_at_with_a_comma_fraction_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.expiresAt (written "2099-01-01T00:00:00,5Z")]: {"id":"urn:ngsi-ld:Subscription:zqns-1","type":"Subscription","entities":[{"type":"Vehicle_zqns"}],"notification":{"endpoint":{"uri":"http://zqsink.invalid/s1","accept":"application/json"}},"expiresAt":"2099-01-01T00:00:00.5Z"}), so `expiresAt` is written as text.
    let expires_at: RawJson = r#""2099-01-01T00:00:00,5Z""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("expiresAt", &expires_at);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        subscription_2.expires_at().map(DateTime::timestamp),
        some(eq("2099-01-01T00:00:00.5Z".parse::<DateTime>()?.timestamp()))
    );
    Ok(())
}
