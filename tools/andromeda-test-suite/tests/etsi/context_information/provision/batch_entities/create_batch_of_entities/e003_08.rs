//! ETSI `003_08`: a batch sent as `application/ld+json` without an `@context` in its body is refused for that entity
//! (clause 6.3.5).

use crate::{
    suite_keywords::{expect_bad_request_entity, partial_batch_answer},
    suite_payloads::building_simple_attributes,
};
use andromeda_test_suite::{broker, namespace::Namespace, wire::content_type};
use googletest::prelude::*;
use ngsi_ld::{http::operation::BatchCreate, model::APPLICATION_LD_JSON};

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/ld+json" and the request payload body does not contain a @context term
/// Clause: 6.3.5
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e003_08_01_create_a_batch_of_one_entity_not_containing_a_json_ld_context_with_a_json_ld_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entities = [building_simple_attributes(&namespace, 1)?];
    let operation = BatchCreate::builder().entities(&entities).build();
    let request = content_type(broker.unchecked(&operation)?, &APPLICATION_LD_JSON)?;
    let result = partial_batch_answer(&broker, request).await?;
    expect_bad_request_entity(&result)
}
