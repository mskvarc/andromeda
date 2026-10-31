//! Equal weights are broken by the order of the clause.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::accept};
use googletest::prelude::*;
use http::{StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{http::operation::QueryEntities, model::Lossless, query::builders::SelectorTypeBuilder};

/// Pins: Query Entities accepting application/ld+json and application/json at equal weight, in either order and
/// also beside application/geo+json, answers application/json, and a lower weight on application/json answers
/// application/ld+json
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_9012_equal_weights_are_broken_by_the_order_of_the_clause() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    let query = QueryEntities::builder().entity_type(&selector).format(Lossless::Normalized).build();
    // The Accept lists several media types at once, which no typed operation sends, so it replaces the query's own.
    let request = accept(broker.unchecked(&query)?, "application/ld+json, application/json");
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK).with_header(CONTENT_TYPE, "application/json"));
    // The Accept lists several media types at once, which no typed operation sends, so it replaces the query's own.
    let request_2 = accept(broker.unchecked(&query)?, "application/json, application/ld+json");
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::OK).with_header(CONTENT_TYPE, "application/json"));
    // The Accept lists several media types at once, which no typed operation sends, so it replaces the query's own.
    let request_3 = accept(broker.unchecked(&query)?, "application/geo+json, application/ld+json, application/json");
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::OK).with_header(CONTENT_TYPE, "application/json"));
    // The Accept weighs its media types, which no typed operation sends, so it replaces the query's own.
    let request_4 = accept(broker.unchecked(&query)?, "application/json;q=0.5, application/ld+json");
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::OK).with_header(CONTENT_TYPE, "application/ld+json"));
    Ok(())
}
