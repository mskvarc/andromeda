//! The typed outcome of a request sent unchecked: a test that must send a request no operation writes (a parameter
//! the operation leaves out, a query without its temporal query) still reads a success answer through the operation
//! the request was started from, so the answer is held to the same model as a typed call.

use bytes::Bytes;
use http::{Request, Response};
use ngsi_ld::http::{ResponseContext, ResponseError, operation::Operation};

/// The outcome `operation` reads from `response`, the broker's answer to `request`, which was started from
/// `operation` and then changed. A test keeps a copy of the request (`UncheckedRequest::as_request`) before
/// sending it, which consumes it.
///
/// # Errors
///
/// Returns [`ResponseError`] for an error response and for a success the operation does not define.
pub fn read_as<O: Operation>(operation: &O, request: &Request<Bytes>, response: Response<Bytes>) -> Result<O::Outcome, ResponseError> {
    operation.decode(&ResponseContext::new(request), response)
}

#[cfg(test)]
mod tests {
    use crate::unchecked_outcome::read_as;
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{Response, StatusCode};
    use ngsi_ld::{
        http::{Endpoint, UncheckedRequest, operation::RetrieveEntityMap},
        registration::EntityMapId,
    };
    use url::Url;

    const MAP: &str = concat!(
        r#"{"id":"urn:ngsi-ld:EntityMap:1","type":"EntityMap","expiresAt":"2030-01-01T00:00:00Z","#,
        r#""entityMap":{"urn:ngsi-ld:Vehicle:A4567":["@none"]},"linkedMaps":{}}"#
    );

    fn started() -> Result<(EntityMapId, Endpoint)> {
        let id = "urn:ngsi-ld:EntityMap:1".parse::<EntityMapId>()?;
        let endpoint = Endpoint::builder().base_url(Url::parse("http://broker.test/ngsi-ld/v1/")?).build()?;
        Ok((id, endpoint))
    }

    #[gtest]
    fn a_success_answer_reads_through_the_operation_the_request_started_from() -> Result<()> {
        let (id, endpoint) = started()?;
        let operation = RetrieveEntityMap { id: &id };
        let request = UncheckedRequest::try_from_operation(&operation, &endpoint)?;
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Bytes::from_static(MAP.as_bytes()))?;
        let map = read_as(&operation, request.as_request(), response)?;
        verify_that!(map.id, eq(&id))
    }

    #[gtest]
    fn an_error_answer_is_refused() -> Result<()> {
        let (id, endpoint) = started()?;
        let operation = RetrieveEntityMap { id: &id };
        let request = UncheckedRequest::try_from_operation(&operation, &endpoint)?;
        let response = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "application/json")
            .body(Bytes::from_static(
                br#"{"type":"https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound","title":"x","detail":"y"}"#,
            ))?;
        verify_that!(read_as(&operation, request.as_request(), response), err(anything()))
    }
}
