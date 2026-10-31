//! Query Temporal Evolution of Entities written down to the byte, for a refusal the typed builders cannot state: an
//! ordering by a member other than `id`, a query too wide to send, a `limit` of 0 without `count`. The answer is read
//! through the typed client, so an error response comes back as a problem to match with [`crate::problem`].

use crate::{
    broker::BrokerClient,
    raw_call::target,
    wire::{WireError, request_to},
};
use bytes::Bytes;
use http::{
    HeaderValue,
    Method,
    Response,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::client::{ClientError, ReqwestError};

/// Why a written query could not be sent.
#[derive(Debug, thiserror::Error)]
pub enum WrittenQueryError {
    /// The target is not a request target.
    #[error(transparent)]
    Wire(#[from] WireError),
}

/// The answer to a written query: the response as received, or the client's error for an error response.
pub type WrittenAnswer = Result<Response<Bytes>, ClientError<ReqwestError>>;

/// Sends `GET /temporal/entities?<query>` (the query string as written, already encoded) accepting `accept`.
///
/// # Errors
///
/// Returns [`WrittenQueryError`] when the query does not make a request target.
pub async fn get(client: &BrokerClient, query: &str, accept: &'static str) -> Result<WrittenAnswer, WrittenQueryError> {
    let request = request_to(client, Method::GET, &format!("temporal/entities?{query}"))?.append_header(ACCEPT, HeaderValue::from_static(accept));
    Ok(client.send_unchecked(request).await)
}

/// Sends `GET /temporal/entities` with `parameters` as its query string, each name and value percent-encoded, in
/// order, accepting `accept`.
///
/// # Errors
///
/// Returns [`WrittenQueryError`] when the parameters do not make a request target.
pub async fn get_parameters(client: &BrokerClient, parameters: &[(&str, &str)], accept: &'static str) -> Result<WrittenAnswer, WrittenQueryError> {
    let request = request_to(client, Method::GET, &target("temporal/entities", parameters))?.append_header(ACCEPT, HeaderValue::from_static(accept));
    Ok(client.send_unchecked(request).await)
}

/// Sends `POST /temporal/entityOperations/query?<query>` (the query string as written, empty for none) with `body`
/// byte for byte under `content_type`, accepting `accept`.
///
/// # Errors
///
/// Returns [`WrittenQueryError`] when the query does not make a request target.
pub async fn post(
    client: &BrokerClient,
    query: &str,
    body: &str,
    content_type: &'static str,
    accept: &'static str,
) -> Result<WrittenAnswer, WrittenQueryError> {
    let target = if query.is_empty() {
        "temporal/entityOperations/query".to_owned()
    } else {
        format!("temporal/entityOperations/query?{query}")
    };
    let request = request_to(client, Method::POST, &target)?
        .append_header(ACCEPT, HeaderValue::from_static(accept))
        .append_header(CONTENT_TYPE, HeaderValue::from_static(content_type))
        .body(Bytes::copy_from_slice(body.as_bytes()));
    Ok(client.send_unchecked(request).await)
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::client_at,
        written_temporal_query::{get, get_parameters, post},
    };
    use googletest::prelude::*;
    use http::StatusCode;
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{body_string, header, method, query_param},
    };

    #[gtest]
    #[tokio::test]
    async fn a_get_goes_out_with_its_query_as_written() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(query_param("orderBy", "name"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let answer = get(&client, "type=T&orderBy=name", "application/json").await?;
        verify_that!(answer.map(|response| response.status()).ok(), some(eq(StatusCode::OK)))
    }

    #[gtest]
    #[tokio::test]
    async fn parameters_are_encoded_in_order() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(query_param("idPattern", "urn:x:.*"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let answer = get_parameters(&client, &[("idPattern", "urn:x:.*")], "application/json").await?;
        verify_that!(answer.map(|response| response.status()).ok(), some(eq(StatusCode::OK)))
    }

    #[gtest]
    #[tokio::test]
    async fn a_post_goes_out_with_its_body_and_content_type_as_written() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(query_param("local", "true"))
            .and(header("content-type", "application/ld+json"))
            .and(body_string(r#"{ "type": "Query" }"#))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let answer = post(&client, "local=true", r#"{ "type": "Query" }"#, "application/ld+json", "application/json").await?;
        verify_that!(answer.map(|response| response.status()).ok(), some(eq(StatusCode::OK)))
    }
}
