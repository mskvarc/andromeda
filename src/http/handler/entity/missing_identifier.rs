//! HTTP handling for the missing identifier entity operation.

use crate::http::handler::entity::{AndromedaError, NgsiLdError, OriginalUri, Response};

pub async fn patch_no_id(OriginalUri(uri): OriginalUri) -> Result<Response, NgsiLdError> {
    Err(NgsiLdError::from(
        AndromedaError::entity_validation("Entity id is required").with_instance(&uri),
    ))
}

pub async fn put_no_id(OriginalUri(uri): OriginalUri) -> Result<Response, NgsiLdError> {
    Err(NgsiLdError::from(
        AndromedaError::entity_validation("Entity id is required").with_instance(&uri),
    ))
}
