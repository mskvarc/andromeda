//! A put without an entity id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: a PUT on the entities collection, which names no Entity id, is refused with 400 `BadRequestData`
/// Clause: 5.6.18.4, 6.4.3
#[gtest]
#[tokio::test]
async fn n_ent_rpl_0004_a_put_without_an_entity_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // No operation writes a PUT on the collection, so the request is sent as written.
    let request = json_request_to(
        &broker,
        Method::PUT,
        "entities",
        &format!(
            "{{\n    \"type\": \"Building_{namespace}\",\n    \"name\": {{\n        \"type\": \"Property\",\n        \"value\": \"Pisa Tower\"\n    }}\n}}"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
