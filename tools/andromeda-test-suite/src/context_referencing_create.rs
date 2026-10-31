//! Create Entity with a bare attribute value under an `@context` array naming one reference, written to the byte.
//!
//! The typed entity model writes an attribute as an object and refuses a relative reference inside an `@context`
//! array, so a test whose subject is how the broker resolves such a reference sends the document as text.

use crate::{
    broker::BrokerClient,
    problem::answered,
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{APPLICATION_LD_JSON, EntityId, EntityTypeName};

/// The document of an entity of `entity_type` with the bare value `"a value"` for `A1`, whose `@context` is the array
/// holding `reference`.
fn document(id: &EntityId, entity_type: &EntityTypeName, reference: &str) -> String {
    format!(r#"{{"id": "{id}", "type": "{entity_type}", "A1": "a value", "@context": ["{reference}"]}}"#)
}

/// Creates the entity `id` of `entity_type` under `Content-Type: application/ld+json` with `A1` set to the bare value
/// `"a value"` and `reference` (an absolute URL, or a path relative to the broker's origin) as its only `@context`
/// entry, and requires 201.
///
/// # Errors
///
/// Fails when the request cannot be written or the broker answers anything but 201.
pub async fn create(client: &BrokerClient, id: &EntityId, entity_type: &EntityTypeName, reference: &str) -> Result<()> {
    let request = content_type(request_to(client, Method::POST, "entities")?, &APPLICATION_LD_JSON)?.body(Bytes::from(document(id, entity_type, reference)));
    verify_that!(&client.send_unchecked(request).await, answered(StatusCode::CREATED))
}

#[cfg(test)]
mod tests {
    use crate::context_referencing_create::document;
    use googletest::prelude::*;
    use ngsi_ld::model::{EntityId, EntityTypeName};

    #[gtest]
    fn the_document_names_the_reference_as_the_only_context_entry() -> Result<()> {
        let text = document(
            &"urn:ngsi-ld:Testing:x-one".parse::<EntityId>()?,
            &"Testing_x".parse::<EntityTypeName>()?,
            "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a1",
        );
        verify_that!(
            text,
            eq(
                r#"{"id": "urn:ngsi-ld:Testing:x-one", "type": "Testing_x", "A1": "a value", "@context": ["/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a1"]}"#
            )
        )
    }
}
