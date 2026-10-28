//! ETSI `003_06`: a batch sent as `application/json` whose entity carries a JSON-LD `@context` is refused for
//! that entity (clause 6.3.5).

use crate::{
    suite_keywords::{expect_bad_request_entity, ld_json_body_client, partial_batch_answer},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary, wire::content_type};
use googletest::prelude::*;
use ngsi_ld::http::operation::BatchCreate;

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/json" and the request payload body (as JSON) contains a "@context" term
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e003_06_01_create_a_batch_of_one_entity_containing_a_json_ld_context_with_a_json_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let entities = [building_simple_attributes(&namespace, 1)?];
    let operation = BatchCreate::builder().entities(&entities).build();
    let request = content_type(client.unchecked(&operation)?, "application/json");
    let result = partial_batch_answer(&client, request).await?;
    expect_bad_request_entity(&result)
}
