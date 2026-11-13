//! A receiver info key naming a header the broker writes itself is bad request data and stores nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    reserved_receiver_info::RESERVED_RECEIVER_INFO,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with an http endpoint whose `receiverInfo` holds a custom header and a key of
/// `Content-Type`, `Content-Length`, `Transfer-Encoding`, `Host`, `Connection`, `TE`, `Trailer`, `Upgrade` or
/// `Keep-Alive`, in any letter case, answers 400 `BadRequestData` naming `receiverInfo` and stores nothing, while the
/// custom header alone answers 201
/// Clause: 5.2.15, 6.3.8, 5.8.1.4, Table 5.2.22-1
/// Recorded: no clause reserves a `receiverInfo` key, so this is stricter than the specification asks; the broker and
/// its HTTP client write each of these fields on every notification, and an entry naming one would send a second
/// `Content-Type`, a length that does not match the body, two framings or a field of the connection the client manages
#[gtest]
#[tokio::test]
async fn a_sub_cre_9031_a_receiver_info_key_naming_a_header_the_broker_writes_itself_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let receiver_type = namespace.entity_type("Receiver")?;
    for (n, (key, value)) in RESERVED_RECEIVER_INFO.into_iter().enumerate() {
        let subscription_id = namespace.subscription_id(n)?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([receiver_type.as_str()])
            .notification(
                NotificationParams::builder(
                    Endpoint::builder(sink.endpoint(n)?)
                        .accept(NgsiLdMediaType::Json)
                        .receiver_info("Authorization", "Bearer abc")
                        .receiver_info(key, value)
                        .build()?,
                )
                .build()?,
            )
            .build()?;
        let answer = broker.with_responses().subscriptions().create(&subscription).send().await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("receiverInfo")
        );
        let retrieved = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await;
        expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    }
    let custom_id = namespace.subscription_id("custom")?;
    let custom = Subscription::builder()
        .id(custom_id.clone())
        .entities([receiver_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("custom")?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Authorization", "Bearer abc")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&custom).send().await?;
    broker.subscriptions().delete(&custom_id).send().await?;
    Ok(())
}
