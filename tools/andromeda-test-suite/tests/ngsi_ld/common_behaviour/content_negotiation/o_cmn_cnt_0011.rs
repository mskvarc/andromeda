//! Retrieve subscription accepting geojson is 406 listing json and json ld.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    not_acceptable::not_acceptable,
    notification_sink::NotificationSink,
    wire::accept,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::RetrieveSubscription,
    model::NgsiLdMediaType,
    subscription::{Endpoint, EntitySubscription, NotificationParams, Subscription},
};

/// Pins: Retrieve Subscription with Accept application/geo+json answers 406 listing the available representations
/// application/json, application/ld+json
/// Clause: 6.3.4, 6.3.15, 5.8.3
/// Recorded: 6.3.4 mandates the 406 and a body containing "the list of the available representations of the
/// resources" without fixing its shape; Andromeda answers `availableRepresentations`, the media types the endpoint
/// renders, in this order
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_0011_retrieve_subscription_accepting_geojson_is_406_listing_json_and_json_ld() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let request = accept(
        broker.unchecked(&RetrieveSubscription::builder().id(&subscription_id).kind(EntitySubscription).build())?,
        "application/geo+json",
    );
    let response = broker.send_unchecked(request).await;
    let (_, body) = not_acceptable(&response).or_fail()?;
    expect_that!(body, json_count("$.availableRepresentations", 2));
    expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    Ok(())
}
