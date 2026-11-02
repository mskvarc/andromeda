//! A subscription geoq over the budget is 413.

use andromeda_test_suite::{
    broker,
    budget::Budget,
    config::SuiteConfig,
    generated_payload::{json_string, polygon_coordinates, ring_polygon},
    namespace::Namespace,
    problem::{problem, problem_status, request_entity_too_large},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::geoquery,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose `geoQ` Polygon is one position over the budget, its coordinates as an array or
/// as a JSON string, answers 413 `RequestEntityTooLarge` naming the budget and `geoQ`, and creates no Subscription
/// Clause: 6.3.2, 5.8.1
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_lim_pay_9008_a_subscription_geoq_over_the_budget_is_413() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let budget = config.budget(Budget::Position)?;
    let probe_type = namespace.entity_type("Probe")?;
    let endpoint = config.unreachable_origin().join("notify")?;
    let array_id = namespace.subscription_id("geoq-array")?;
    let string_id = namespace.subscription_id("geoq-string")?;
    let as_array = Subscription::builder()
        .id(array_id.clone())
        .entities([probe_type.as_str()])
        .geo_q(geoquery::intersects(ring_polygon(budget + 1)?).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(endpoint.clone()).build()?).build()?)
        .build()?;
    let refused = broker.with_responses().subscriptions().create(&as_array).send().await;
    expect_that!(
        &refused,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing(&budget.to_string())
            .with_detail_containing("geoQ")
    );
    expect_that!(problem_status(&refused), some(eq(413)));
    // The coordinates are written as a JSON string holding the array, which the geoquery model does not write.
    let coordinates = json_string(&polygon_coordinates(budget + 1)?);
    let body = format!(
        r#"{{"id":"{string_id}","type":"Subscription","entities":[{{"type":"{probe_type}"}}],"geoQ":{{"geometry":"Polygon","coordinates":{coordinates},"georel":"intersects"}},"notification":{{"endpoint":{{"uri":"{endpoint}"}}}}}}"#
    );
    let as_string = broker.send_unchecked(json_request_to(&broker, Method::POST, "subscriptions", &body)?).await;
    expect_that!(
        &as_string,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing(&budget.to_string())
            .with_detail_containing("geoQ")
    );
    expect_that!(problem_status(&as_string), some(eq(413)));
    for id in [&array_id, &string_id] {
        let absent = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    Ok(())
}
