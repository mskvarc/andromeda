//! A request body whose `@context` names a stored context by a reference relative to the broker's origin, the form
//! Add @context answers in its `Location` (clause 5.13.2.4).
//!
//! The client refuses to write a relative reference into a body, since nothing resolves it for the broker there. The
//! request is built by the typed operation with its body delivery, which writes the body and a core `@context`, and
//! the `@context` member of every document of the body is then replaced by the reference as the test states it.

use crate::unchecked_document::{UncheckedDocumentError, from_rendered};
use ngsi_ld::{
    http::{UncheckedDocument, UncheckedRequest},
    model::{InvalidJson, RawJson},
};
use serde_json::{Value, value::RawValue};

/// The member holding a document's `@context`.
const CONTEXT: &str = "@context";

/// Why the `@context` of a body could not be replaced.
#[derive(Debug, thiserror::Error)]
pub enum RelativeContextError {
    /// The body of the operation is not JSON.
    #[error("the request body is not JSON")]
    NotJson(#[source] serde_json::Error),
    /// The body is neither a document nor an array of documents.
    #[error("the request body is neither a document nor an array of documents")]
    Shape,
    /// A document of the body cannot be carried into an unchecked document.
    #[error(transparent)]
    Document(#[from] UncheckedDocumentError),
    /// The context text is not JSON the model reads back.
    #[error("the context is not JSON the model reads back")]
    Context(#[source] InvalidJson),
}

/// `request` with the `@context` of its body, a document or an array of documents, replaced by `context`; a document
/// without one gets it appended.
///
/// # Errors
///
/// Returns [`RelativeContextError`] when the body is not a JSON document or an array of documents.
pub fn with_context_text(request: UncheckedRequest, context: &Value) -> Result<UncheckedRequest, RelativeContextError> {
    let body: Box<RawValue> = serde_json::from_slice(request.as_request().body()).map_err(RelativeContextError::NotJson)?;
    let raw: RawJson = context.to_string().parse().map_err(RelativeContextError::Context)?;
    // The documents are read as their text, so each keeps its members in the order the operation wrote them.
    match body.get().as_bytes().first() {
        Some(b'{') => Ok(request.document(&replaced(&body, &raw)?)),
        Some(b'[') => {
            let documents: Vec<Box<RawValue>> = serde_json::from_str(body.get()).map_err(RelativeContextError::NotJson)?;
            let replaced: Vec<UncheckedDocument> = documents.iter().map(|document| replaced(document, &raw)).collect::<Result<_, _>>()?;
            Ok(request.documents(&replaced))
        }
        Some(_) | None => Err(RelativeContextError::Shape),
    }
}

/// The document `body` with its `@context` set to `context`.
fn replaced(body: &RawValue, context: &RawJson) -> Result<UncheckedDocument, RelativeContextError> {
    let rendered: RawJson = body.get().parse().map_err(|_| RelativeContextError::Shape)?;
    Ok(from_rendered(&rendered)?.set_member(CONTEXT, context))
}

#[cfg(test)]
mod tests {
    use crate::relative_context::with_context_text;
    use bytes::Bytes;
    use googletest::prelude::*;
    use iri_rs::iri;
    use ngsi_ld::{
        http::{ContextDelivery, Endpoint, UncheckedRequest, operation::BatchCreate},
        model::Entity,
    };
    use serde_json::json;
    use std::str::from_utf8;
    use url::Url;

    fn batch_create() -> Result<UncheckedRequest> {
        let vehicles = [Entity::builder()
            .id(iri!("urn:ngsi-ld:Vehicle:A"))
            .entity_type("Vehicle")
            .property("speed", 1)
            .build()?];
        let endpoint = Endpoint::builder()
            .base_url(Url::parse("http://broker.test/ngsi-ld/v1/")?)
            .context_delivery(ContextDelivery::LdJsonBody)
            .build()?;
        Ok(UncheckedRequest::try_from_operation(
            &BatchCreate::builder().entities(&vehicles).build(),
            &endpoint,
        )?)
    }

    #[gtest]
    fn a_relative_reference_replaces_the_context_of_every_document_in_place() -> Result<()> {
        let request = with_context_text(batch_create()?, &json!("/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a"))?;
        verify_that!(
            request.as_request().body(),
            eq(&Bytes::from_static(
                br#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle","speed":{"type":"Property","value":1},"@context":"/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a"}]"#
            ))
        )
    }

    #[gtest]
    fn a_context_array_replaces_the_context_as_written() -> Result<()> {
        let request = with_context_text(batch_create()?, &json!(["/ngsi-ld/v1/jsonldContexts/a"]))?;
        verify_that!(
            from_utf8(request.as_request().body())?,
            ends_with(r#""@context":["/ngsi-ld/v1/jsonldContexts/a"]}]"#)
        )
    }
}
