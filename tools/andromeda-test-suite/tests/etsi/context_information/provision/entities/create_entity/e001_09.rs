//! ETSI `001_09`: an entity sent with a JSON-LD content type and a `Link` header naming its `@context` is refused
//! (clause 6.3.5).

use crate::{
    suite_keywords::{context_link, ld_json_body_client, refused},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary, wire::content_type};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::operation::CreateEntity,
    model::{APPLICATION_LD_JSON, ErrorType},
};

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/ld+json" and a JSON-LD Link header is present in the incoming HTTP request
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e001_09_01_link_header_with_json_ld_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let request = content_type(
        client
            .unchecked(&CreateEntity::builder().entity(&entity).build())?
            .append_header(LINK, context_link(&vocabulary)?),
        &APPLICATION_LD_JSON,
    )?;
    refused(&client, request, ErrorType::BadRequestData).await?;
    Ok(())
}
