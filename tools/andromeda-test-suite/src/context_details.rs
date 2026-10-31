//! The metadata a broker reports about its stored `@context`s (clause 5.13.3.5), read from the answer of Serve
//! @context and List @contexts with `details=true`.
//!
//! The crate's typed reads refuse an `expiresAt` that is the JSON null, which a stored context with no expiry is
//! answered with. The request is the crate's own operation sent unchecked, and the answer is read into the crate's
//! [`ContextMetadata`] once the null `expiresAt` is taken out, the member a context without expiry has no value for.

use crate::{broker::BrokerClient, problem::answered};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Response, StatusCode};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::operation::{ContextKindFilter, ListContextDetails, ServeContextDetails},
    model::{ContextId, ContextMetadata, DocumentError},
};
use serde_json::Value;

/// Why an answer is not the metadata of stored contexts.
#[derive(Debug, thiserror::Error)]
pub enum ContextDetailsError {
    /// The body is not JSON.
    #[error("the body is not JSON")]
    NotJson(#[source] serde_json::Error),
    /// The body of List @contexts is not an array.
    #[error("the body is not an array")]
    NotArray,
    /// The body of Serve @context, or an item of a list, is not an object.
    #[error("the body is not an object")]
    NotObject,
    /// A metadata object is refused by the crate's reader.
    #[error("a metadata object is not valid")]
    Metadata(#[source] DocumentError),
}

/// Serve @context with `details=true` of the stored context `id`, answered as received.
///
/// # Errors
///
/// Returns [`ClientError`] when the request cannot be built, the transport fails or the broker answers with an error.
pub async fn serve_details(client: &BrokerClient, id: &ContextId) -> Result<Response<Bytes>, ClientError<ReqwestError>> {
    client.send_unchecked(client.unchecked(&ServeContextDetails { id })?).await
}

/// List @contexts with `details=true`, of `kind` only when it is given, answered as received.
///
/// # Errors
///
/// Returns [`ClientError`] when the request cannot be built, the transport fails or the broker answers with an error.
pub async fn list_details(client: &BrokerClient, kind: Option<ContextKindFilter>) -> Result<Response<Bytes>, ClientError<ReqwestError>> {
    client
        .send_unchecked(client.unchecked(&ListContextDetails::builder().maybe_kind(kind).build())?)
        .await
}

/// The metadata of the stored context `id` a Serve @context with `details=true` answers with 200.
///
/// # Errors
///
/// Fails when the broker answers anything but 200 or a body that is not context metadata.
pub async fn served(client: &BrokerClient, id: &ContextId) -> Result<ContextMetadata> {
    let answer = serve_details(client, id).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    Ok(metadata(answer.as_ref().or_fail()?)?)
}

/// The metadata of every context a List @contexts with `details=true` of `kind` answers with 200, in the order listed.
///
/// # Errors
///
/// Fails when the broker answers anything but 200 or a body that is not a list of metadata.
pub async fn listed(client: &BrokerClient, kind: Option<ContextKindFilter>) -> Result<Vec<ContextMetadata>> {
    let answer = list_details(client, kind).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    Ok(metadata_list(answer.as_ref().or_fail()?)?)
}

/// The metadata one answer of Serve @context holds.
///
/// # Errors
///
/// Returns [`ContextDetailsError`] when the body is not one valid metadata object.
pub fn metadata(response: &Response<Bytes>) -> Result<ContextMetadata, ContextDetailsError> {
    read(parse(response)?)
}

/// The metadata of every context one answer of List @contexts holds, in the order listed.
///
/// # Errors
///
/// Returns [`ContextDetailsError`] when the body is not an array of valid metadata objects.
pub fn metadata_list(response: &Response<Bytes>) -> Result<Vec<ContextMetadata>, ContextDetailsError> {
    match parse(response)? {
        Value::Array(items) => items.into_iter().map(read).collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => Err(ContextDetailsError::NotArray),
    }
}

/// The body of `response` as JSON.
fn parse(response: &Response<Bytes>) -> Result<Value, ContextDetailsError> {
    serde_json::from_slice(response.body()).map_err(ContextDetailsError::NotJson)
}

/// The metadata of one JSON object, without its null `expiresAt`.
fn read(value: Value) -> Result<ContextMetadata, ContextDetailsError> {
    match value {
        Value::Object(mut members) => {
            if members.get("expiresAt").is_some_and(Value::is_null) {
                members.remove("expiresAt");
            }
            ContextMetadata::parse(&Value::Object(members).to_string()).map_err(ContextDetailsError::Metadata)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Err(ContextDetailsError::NotObject),
    }
}

#[cfg(test)]
mod tests {
    use crate::context_details::{metadata, metadata_list};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;
    use ngsi_ld::model::StoredContextKind;

    const HOSTED: &str = r#"{"URL":"http://broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a1","localId":"urn:ngsi-ld:JsonLdContext:a1","kind":"Hosted","createdAt":"2024-05-01T10:00:00Z","expiresAt":null,"lastUsage":"2024-05-01T10:00:00Z","numberOfHits":2}"#;

    fn answer(text: &str) -> Response<Bytes> {
        Response::new(Bytes::from(text.to_owned()))
    }

    #[gtest]
    fn a_null_expiry_is_read_as_no_expiry() -> Result<()> {
        let read = metadata(&answer(HOSTED))?;
        expect_that!(read.kind, eq(&StoredContextKind::Hosted));
        expect_that!(read.expires_at, none());
        verify_that!(read.number_of_hits, some(eq(2)))
    }

    #[gtest]
    fn a_list_is_read_object_by_object() -> Result<()> {
        let read = metadata_list(&answer(&format!("[{HOSTED},{HOSTED}]")))?;
        verify_that!(read.len(), eq(2))
    }

    #[gtest]
    fn a_body_of_another_shape_is_refused() {
        expect_that!(metadata(&answer("[]")).is_err(), eq(true));
        expect_that!(metadata_list(&answer("{}")).is_err(), eq(true));
        expect_that!(metadata(&answer("not json")).is_err(), eq(true));
        expect_that!(metadata(&answer(r#"{"kind":"Hosted"}"#)).is_err(), eq(true));
    }
}
