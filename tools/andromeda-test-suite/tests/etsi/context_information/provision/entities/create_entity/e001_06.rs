//! ETSI `001_06`: a create sent as `application/json` whose body carries an `@context` member is refused with 400
//! `BadRequestData` (clause 6.3.5).

use crate::{
    suite_keywords::{ld_json_body_client, refused},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    vocabulary::Vocabulary,
    wire::{api_uri, content_type},
};
use googletest::prelude::*;
use ngsi_ld::{http::operation::CreateEntity, model::ErrorType};

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/json" and the request payload body (as JSON) contains a "@context" term
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e001_06_01_payload_context_with_json_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let create = CreateEntity::builder().entity(&entity).build();
    let request = content_type(client.unchecked(&create)?, "application/json").uri(api_uri(&client, "entities/")?);
    refused(&client, request, ErrorType::BadRequestData).await
}
