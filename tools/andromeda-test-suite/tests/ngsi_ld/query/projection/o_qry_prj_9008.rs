//! Attrs beside pick or omit in a query body is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::operation::QueryMethod, model::ErrorType, query::projection::Projection};

/// Pins: Query Entities by POST with attrs beside pick or beside omit answers 400 `BadRequestData`
/// Clause: 4.21, 5.2.23, 6.23.3.1
/// Recorded: no clause forbids combining pick or omit with the deprecated attrs, which Tables 6.4.3.2-1, 6.5.3.1-1
/// and 5.2.14.1-1 make a synonym for pick (attributes in a notification); Andromeda refuses attrs beside pick or
/// omit with `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_qry_prj_9008_attrs_beside_pick_or_omit_in_a_query_body_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let answer = broker
        .with_responses()
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&namespace.entity_type("Vehicle")?)
        .pick("speed".parse::<Projection>()?)
        .attrs(vec!["speed".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_2 = broker
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&namespace.entity_type("Vehicle")?)
        .omit("speed".parse::<Projection>()?)
        .attrs(vec!["speed".parse()?])
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
