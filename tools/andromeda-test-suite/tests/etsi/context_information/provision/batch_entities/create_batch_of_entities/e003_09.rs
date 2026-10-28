//! ETSI `003_09`: a batch sent as `application/ld+json` with a `Link` header naming a context is refused as a whole
//! (clause 6.3.5).

use crate::{
    suite_keywords::{context_link, ld_json_body_client, refused},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{http::operation::BatchCreate, model::ErrorType};

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/ld+json" and a JSON-LD Link header is present in the incoming HTTP request
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e003_09_01_create_a_batch_of_one_entity_with_a_link_header_and_a_json_ld_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let entities = [building_simple_attributes(&namespace, 1)?];
    let operation = BatchCreate::builder().entities(&entities).build();
    let request = client.unchecked(&operation)?.append_header(LINK, context_link(&vocabulary)?);
    refused(&client, request, ErrorType::BadRequestData).await
}
