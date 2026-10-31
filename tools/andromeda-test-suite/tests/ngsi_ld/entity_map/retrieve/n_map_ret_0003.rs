//! An identifier that is no uri is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RetrieveEntityMap},
    model::{ErrorType, NgsiLdMediaType},
    registration::EntityMapId,
};

/// Pins: Retrieve `EntityMap` of an identifier that is not a URI answers 400 `BadRequestData`
/// Clause: 5.14.1.4, 6.32.3.1
#[gtest]
#[tokio::test]
async fn n_map_ret_0003_an_identifier_that_is_no_uri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?.with_accept(Accept::new(NgsiLdMediaType::Json));
    let placeholder_id = format!("urn:ngsi-ld:EntityMap:{namespace}-placeholder").parse::<EntityMapId>()?;
    // The model refuses an identifier that is no URI; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(&RetrieveEntityMap { id: &placeholder_id })?
        .uri(api_uri(&broker, "entityMaps/not-a-valid-uri")?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
