//! An application media range expands to json.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::accept};
use googletest::prelude::*;
use http::{StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{http::operation::QueryEntities, model::Lossless, query::builders::SelectorTypeBuilder};

/// Pins: Query Entities accepting application/* or */* answers application/json, and accepting text/* answers 406
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_9013_an_application_media_range_expands_to_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    let query = QueryEntities::builder().entity_type(&selector).format(Lossless::Normalized).build();
    // The Accept is a media range, which no typed operation sends, so it replaces the query's own.
    let request = accept(broker.unchecked(&query)?, "application/*");
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK).with_header(CONTENT_TYPE, "application/json"));
    // The Accept is a media range, which no typed operation sends, so it replaces the query's own.
    let request_2 = accept(broker.unchecked(&query)?, "*/*");
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::OK).with_header(CONTENT_TYPE, "application/json"));
    // The Accept is a media range, which no typed operation sends, so it replaces the query's own.
    let request_3 = accept(broker.unchecked(&query)?, "text/*");
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}
