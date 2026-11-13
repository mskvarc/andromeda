//! An update with a receiver info key naming a header the broker writes itself is bad request data and keeps the stored
//! one.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::problem,
    reserved_receiver_info::RESERVED_RECEIVER_INFO,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, KeyValuePair, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription with an http endpoint whose `receiverInfo` holds a custom header and a key of
/// `Content-Type`, `Content-Length`, `Transfer-Encoding`, `Host`, `Connection`, `TE`, `Trailer`, `Upgrade` or
/// `Keep-Alive`, in any letter case, answers 400 `BadRequestData` naming `receiverInfo` and keeps the stored
/// `receiverInfo`
/// Clause: 5.2.15, 6.3.8, 5.8.2.4, Table 5.2.22-1
/// Recorded: no clause reserves a `receiverInfo` key, so this is stricter than the specification asks; the broker and
/// its HTTP client write each of these fields on every notification, and an entry naming one would send a second
/// `Content-Type`, a length that does not match the body, two framings or a field of the connection the client manages
#[gtest]
#[tokio::test]
async fn a_sub_upd_9015_an_update_with_a_receiver_info_key_naming_a_header_the_broker_writes_itself_is_bad_request_data_and_keeps_the_stored_one() -> Result<()>
{
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let receiver_type = namespace.entity_type("Receiver")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([receiver_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Authorization", "Bearer abc")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    for (key, value) in RESERVED_RECEIVER_INFO {
        let fragment = SubscriptionFragment::builder()
            .notification(
                NotificationParams::builder(
                    Endpoint::builder(sink.endpoint(1)?)
                        .accept(NgsiLdMediaType::Json)
                        .receiver_info("Authorization", "Bearer other")
                        .receiver_info(key, value)
                        .build()?,
                )
                .build()?,
            )
            .build()?;
        let answer = broker.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("receiverInfo")
        );
        let stored = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        expect_that!(
            stored
                .notification()
                .endpoint()
                .receiver_info()
                .iter()
                .map(|pair| (KeyValuePair::key(pair), KeyValuePair::value(pair)))
                .collect::<Vec<_>>(),
            elements_are![eq(&("Authorization", "Bearer abc"))]
        );
    }
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
