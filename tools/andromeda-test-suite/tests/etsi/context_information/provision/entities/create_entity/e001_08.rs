//! ETSI `001_08`: an entity sent with a JSON-LD content type but no `@context` is refused (clause 6.3.5).

use crate::{suite_keywords::refused, suite_payloads::building_simple_attributes};
use andromeda_test_suite::{broker, namespace::Namespace, wire::content_type};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::CreateEntity,
    model::{APPLICATION_LD_JSON, ErrorType},
};

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/ld+json" and the request payload body does not contain a @context term
/// Clause: 6.3.5
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e001_08_01_no_payload_context_with_json_ld_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entity = building_simple_attributes(&namespace, 1)?;
    let request = content_type(broker.unchecked(&CreateEntity::builder().entity(&entity).build())?, &APPLICATION_LD_JSON)?;
    refused(&broker, request, ErrorType::BadRequestData).await?;
    Ok(())
}
