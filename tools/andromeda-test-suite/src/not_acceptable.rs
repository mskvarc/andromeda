//! The 406 answer to a typed operation sent with an `Accept` no typed operation states (clause 6.3.4), as the client
//! read it: the head and the body exactly as received.

use bytes::Bytes;
use http::Response;
use ngsi_ld::{
    client::ClientError,
    http::{ResponseError, ResponseHead},
};

/// The head and the body of `answer` when the broker refused every representation the request accepts (406, clause
/// 6.3.4); `None` for any other answer. Clause 6.3.4 fixes no shape for the body, so it is kept as received.
#[must_use]
pub fn not_acceptable<E>(answer: &Result<Response<Bytes>, ClientError<E>>) -> Option<(&ResponseHead, &Bytes)> {
    match answer {
        Err(ClientError::Response(ResponseError::NotAcceptable { head, body, .. })) => Some((head, body)),
        Ok(_) | Err(_) => None,
    }
}
