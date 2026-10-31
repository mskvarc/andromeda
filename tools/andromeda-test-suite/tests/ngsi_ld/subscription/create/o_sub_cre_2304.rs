//! A geo query with mixed arity positions is refused.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem_among},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose `geoQ` coordinates mix two- and three-ordinate positions is refused with a 400
/// and stores nothing
/// Clause: 4.7.1, 4.10, 5.8.1
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected on every path that takes a geometry; no clause names the error
/// type, so `BadRequestData` and `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_sub_cre_2304_a_geo_query_with_mixed_arity_positions_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"geometry": "LineString", "coordinates": [[13.4, 52.52], [13.5, 52.53, 250.0]], "georel": "intersects"}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("geoQ", &geo_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
