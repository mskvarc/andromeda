//! A batch delete of an empty array says the array of entity identifiers is empty.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::operation::BatchDelete, model::ErrorType};

/// Pins: Batch Entity Delete with the body `[]` answers 400 `BadRequestData` whose detail says the array of entity
/// identifiers is empty, not that a JSON object was expected
/// Clause: 5.5.4, 5.6.10.3, 5.6.10.4
/// Recorded: clause 5.6.10.4 fixes the error type but not the wording; Andromeda says what the array holds
#[gtest]
#[tokio::test]
async fn o_cmn_val_9045_a_batch_delete_of_an_empty_array_says_the_array_of_entity_identifiers_is_empty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ids = [namespace.urn("Vehicle", 1)?];
    // The client writes no batch without an entity, so the empty array is written as text.
    let request = broker.unchecked(&BatchDelete { ids: &ids })?.body(Bytes::from_static(b"[]"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the array of entity identifiers in the request body is empty")
    );
    Ok(())
}
