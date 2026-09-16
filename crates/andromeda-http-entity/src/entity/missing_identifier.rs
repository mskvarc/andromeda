//! HTTP handling for the missing identifier entity operation.

use crate::entity::{NgsiLdErrorType, OriginalUri, ProblemResponse, Response, public_error};

pub(crate) async fn patch_no_id(OriginalUri(uri): OriginalUri) -> Result<Response, ProblemResponse> {
    Err(public_error(NgsiLdErrorType::BadRequestData, "entity id is required", &uri))
}

pub(crate) async fn put_no_id(OriginalUri(uri): OriginalUri) -> Result<Response, ProblemResponse> {
    Err(public_error(NgsiLdErrorType::BadRequestData, "entity id is required", &uri))
}
