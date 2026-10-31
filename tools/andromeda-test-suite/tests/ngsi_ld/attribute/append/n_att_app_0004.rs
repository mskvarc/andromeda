//! A post without an entity id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::AppendAttributes,
    model::{EntityFragment, ErrorType},
};

/// Pins: Append Attributes addressed to an empty Entity id path segment is refused with 400 `BadRequestData`
/// Clause: 5.6.3.4, 6.6.3.1
#[gtest]
#[tokio::test]
async fn n_att_app_0004_a_post_without_an_entity_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let placeholder_id = namespace.urn("Placeholder", 1)?;
    let fragment = EntityFragment::builder().property("speed", 100).build()?;
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(&AppendAttributes::builder().id(&placeholder_id).fragment(&fragment).build())?
        .uri(api_uri(&broker, "entities//attrs")?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
