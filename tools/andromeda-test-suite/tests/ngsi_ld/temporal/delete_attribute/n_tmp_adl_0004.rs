//! An empty entity id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::DeleteTemporalAttribute,
    model::{AttributeName, ErrorType},
};

/// Pins: Delete Attribute addressed to an empty Entity id path segment is refused with 400 `BadRequestData`, with a
/// title
/// Clause: 5.6.13.4, 6.21.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_adl_0004_an_empty_entity_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let placeholder_id = namespace.urn("Placeholder", 1)?;
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(
            &DeleteTemporalAttribute::builder()
                .id(&placeholder_id)
                .name(&"fuelLevel".parse::<AttributeName>()?)
                .build(),
        )?
        .uri(api_uri(&broker, "temporal/entities//attrs/fuelLevel")?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
