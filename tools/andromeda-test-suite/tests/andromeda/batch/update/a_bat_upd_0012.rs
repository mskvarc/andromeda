//! A respond async update of a million position entity under live delivery is applied.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    config::{BrokerRole, Budget, SuiteConfig},
    eventually::retried,
    generated_payload::polygon,
    namespace::Namespace,
    problem::answered,
    respond_async::{async_applied, respond_async},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    model::EntityId,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a batch update answered with Preference-Applied: respond-async that replaces the 1,000,000-position
/// Polygon of an Entity a Subscription watches, under live delivery, is applied: the Entity reads back with the new
/// Polygon and no request answers 500
/// Clause: 5.6.9.4
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), and the position budget of broker-recorded, raised to 1,000,000 positions a geometry
#[gtest]
#[tokio::test]
async fn a_bat_upd_0012_a_respond_async_update_of_a_million_position_entity_under_live_delivery_is_applied() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_recorded = broker::client(BrokerRole::Recorded)?;
    let config = SuiteConfig::get()?;
    let large_budget = config.budget(Budget::LargePosition)?;
    let big_area_id = format!("urn:ngsi-ld:BigArea:{namespace}").parse::<EntityId>()?;
    let big_area_type = namespace.entity_type("BigArea")?;
    let subscription_id = namespace.subscription_id("big-area")?;
    // The size of the polygon is the subject, so it is drawn by the payload generator and sent as text.
    let location = |positions: u64| -> Result<String> {
        Ok(format!(
            r#""id":"{big_area_id}","type":"{big_area_type}","location":{{"type":"GeoProperty","value":{}}}"#,
            polygon(positions)?
        ))
    };
    let create = request_to(&broker_recorded, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!("{{{}}}", location(large_budget)?)));
    expect_that!(&broker_recorded.send_unchecked(create).await, answered(StatusCode::CREATED));
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([big_area_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(config.unreachable_origin().join("notify")?).build()?).build()?)
        .build()?;
    broker_recorded.subscriptions().create(&subscription).send().await?;
    // Every ring of a generated payload is drawn around one point and starts at the same vertex, so the replacement is
    // told apart by its position count.
    let update = respond_async(
        request_to(&broker_recorded, Method::POST, "entityOperations/update")?
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .body(Bytes::from(format!("[{{{}}}]", location(large_budget - 1)?))),
    );
    expect_that!(&broker_recorded.send_unchecked(update).await, async_applied(StatusCode::NO_CONTENT));
    retried(async || {
        let answer = broker_recorded
            .send_unchecked(request_to(&broker_recorded, Method::GET, &format!("entities/{big_area_id}"))?)
            .await;
        verify_that!(&answer, answered(StatusCode::OK))?;
        let response = answer?;
        // The geometry matcher describes every position of a geometry it refuses, which a million positions make too
        // large to print on each poll, so the answer is read as received.
        let body = response.body();
        verify_that!(body, json_equals("$.location.value.type", "Polygon"))?;
        verify_that!(body, json_count("$.location.value.coordinates[0]", usize::try_from(large_budget - 1)?))?;
        verify_that!(body, json_equals("$.location.value.coordinates[0][0][0]", 14.6))?;
        verify_that!(body, json_equals("$.location.value.coordinates[0][0][1]", 46.05))?;
        Ok(())
    })
    .await?;
    broker_recorded.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
