//! An empty fragment for an unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::{EntityFragment, ErrorType};

/// Pins: Partial Attribute Update, Update Attributes, Append Attributes and Merge Entity with the empty fragment {}
/// on an entity that does not exist each answer 404 `ResourceNotFound`
/// Clause: 5.6.2.4, 5.6.3.4, 5.6.4.4, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9019_an_empty_fragment_for_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_missing_id = namespace.urn("Vehicle", "missing")?;
    // The model writes no empty attribute fragment, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_missing_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let fragment = EntityFragment::builder().build()?;
    let answer_2 = broker
        .with_responses()
        .entities()
        .update_attributes(&vehicle_missing_id, &fragment)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let fragment_2 = EntityFragment::builder().build()?;
    let answer_3 = broker
        .with_responses()
        .entities()
        .append_attributes(&vehicle_missing_id, &fragment_2)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let fragment_3 = EntityFragment::builder().build()?;
    let answer_4 = broker.with_responses().entities().merge(&vehicle_missing_id, &fragment_3).send().await;
    expect_that!(&answer_4, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
