//! An id that is not a uri is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::ReplaceAttribute,
    model::{Attribute, AttributeName, ErrorType, Property},
};

/// Pins: Replace Attribute addressed to an Entity id that is not a URI is refused with 400 `BadRequestData`
/// Clause: 5.6.19.4, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_0006_an_id_that_is_not_a_uri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let placeholder_id = namespace.urn("Placeholder", 1)?;
    let attribute = Attribute::from(Property::builder(100).build()?);
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(
            &ReplaceAttribute::builder()
                .id(&placeholder_id)
                .name(&"speed".parse::<AttributeName>()?)
                .attribute(&attribute)
                .build(),
        )?
        .uri(api_uri(&broker, &format!("entities/invalid-{namespace}/attrs/speed"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
