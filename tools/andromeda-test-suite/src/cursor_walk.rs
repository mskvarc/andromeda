//! The first page of a cursor walk (clause 5.5.9.1): a broker on its defaults pages a request that names neither
//! `offset` nor `cursor` by offset, and an empty `cursor=`, a parameter no operation writes, starts a walk by
//! Andromeda's opaque cursor. The answer is read through the operation the request was started from, so the page and
//! the pointers it offers are the typed ones.

use crate::{broker::BrokerClient, unchecked_outcome::read_as};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{RequestError, ResponseError, ResponseHead, UncheckedRequest, operation::Operation},
};

/// Why the first page of a cursor walk could not be read.
#[derive(Debug, thiserror::Error)]
pub enum CursorWalkError {
    /// The operation refuses its parameters, or the call fails or is refused.
    #[error(transparent)]
    Client(#[from] ClientError<ReqwestError>),
    /// The request with the cursor is not a URI.
    #[error(transparent)]
    Request(#[from] RequestError),
    /// The answer is not a success the operation defines.
    #[error(transparent)]
    Response(#[from] ResponseError),
}

/// The first page of a cursor walk and the header fields it came with.
#[derive(Debug)]
pub struct CursorStart<T> {
    /// The header fields of the answer, its `Link` among them.
    pub head: ResponseHead,
    /// The page, read through the operation.
    pub page: T,
}

/// The request `operation` writes on `client`, with the empty `cursor` that starts a walk added last.
fn started<O: Operation>(client: &BrokerClient, operation: &O) -> Result<UncheckedRequest, CursorWalkError> {
    Ok(client.unchecked(operation)?.undefined_query_parameter("cursor", "")?)
}

/// The first page of the cursor walk of `operation` on `client`, on the client's tenant and with its `Accept`.
///
/// # Errors
///
/// Returns [`CursorWalkError`] when the request cannot be written or sent, the broker refuses it, or the answer is not
/// a page of the operation.
pub async fn first_page<O: Operation + Sync>(client: &BrokerClient, operation: &O) -> Result<CursorStart<O::Outcome>, CursorWalkError> {
    let request = started(client, operation)?;
    // clone: sending consumes the request the answer is read against
    let sent = request.as_request().clone();
    let answer = client.send_unchecked(request).await?;
    // clone: reading the page consumes the answer the fields came with
    let head = ResponseHead::new(answer.headers().clone());
    let page = read_as(operation, &sent, answer)?;
    Ok(CursorStart { head, page })
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, cursor_walk::started};
    use googletest::prelude::*;
    use ngsi_ld::{http::operation::QuerySubscriptions, subscription::EntitySubscription};
    use url::Url;

    #[gtest]
    fn a_walk_starts_with_an_empty_cursor_after_the_parameters_of_the_operation() -> Result<()> {
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let query = QuerySubscriptions::builder().kind(EntitySubscription).limit(2).build();
        let request = started(&client, &query)?;
        verify_that!(
            request.as_request().uri().to_string(),
            eq("http://broker.test/ngsi-ld/v1/subscriptions?limit=2&cursor=")
        )
    }
}
