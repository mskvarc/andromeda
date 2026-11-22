//! A raw plus in a scope query is a space and bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_call::target, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Entities with the query string `scopeQ=/Madrid/+/ParqueNorte`, its plus unencoded, is refused with
/// `BadRequestData`: the plus is read as a space, and a scope level begins with a letter
/// Clause: 4.9, 4.19, 5.7.2, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_scp_4003_a_raw_plus_in_a_scope_query_is_a_space_and_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let type_text = building_type.to_string();
    // The typed query percent-encodes the plus, so this read goes out as written. The query string is
    // form-urlencoded (OpenAPI 3.0.4 Appendix E), where an unencoded plus stands for a space.
    let read = format!("{}&scopeQ=/Madrid/+/ParqueNorte", target("entities", &[("type", type_text.as_str())]));
    let answer = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
