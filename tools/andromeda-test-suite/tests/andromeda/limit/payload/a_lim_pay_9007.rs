//! A query reference geometry over the budget is too complex.

use andromeda_test_suite::{
    broker,
    config::{Budget, SuiteConfig},
    generated_payload::{json_string, polygon_coordinates, ring_polygon},
    namespace::Namespace,
    problem::{problem, problem_status},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{ErrorType, NgsiLdMediaType},
    query::geoquery,
};

/// Pins: Query Entities through POST `/entityOperations/query` with a reference Polygon one position over the
/// budget, its coordinates as an array or as a JSON string, answers 403 `TooComplexQuery` naming the budget
/// Clause: 5.5.6, 6.3.2, 5.7.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_lim_pay_9007_a_query_reference_geometry_over_the_budget_is_too_complex() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::Position)?;
    let probe_type = namespace.entity_type("Probe")?;
    let as_array = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&probe_type)
        .geo_q(geoquery::intersects(ring_polygon(budget + 1)?))
        .send()
        .await;
    expect_that!(
        &as_array,
        problem(StatusCode::FORBIDDEN, ErrorType::TooComplexQuery).with_detail_containing(&budget.to_string())
    );
    expect_that!(problem_status(&as_array), some(eq(403)));
    // The coordinates are written as a JSON string holding the array, which the geoquery model does not write.
    let coordinates = json_string(&polygon_coordinates(budget + 1)?);
    let query = format!(
        r#"{{"type":"Query","entities":[{{"type":"{probe_type}"}}],"geoQ":{{"geometry":"Polygon","coordinates":{coordinates},"georel":"intersects"}}}}"#
    );
    let as_string = broker
        .send_unchecked(json_request_to(&broker, Method::POST, "entityOperations/query", &query)?)
        .await;
    expect_that!(
        &as_string,
        problem(StatusCode::FORBIDDEN, ErrorType::TooComplexQuery).with_detail_containing(&budget.to_string())
    );
    expect_that!(problem_status(&as_string), some(eq(403)));
    Ok(())
}
