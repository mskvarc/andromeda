//! A request sent without the client's reading of the answer, for a test whose subject is the exact spelling of an
//! answer the model reads into its own form, such as an endpoint URL or a member it holds as a property.

use crate::exchange_log::ExchangeLog;
use bytes::Bytes;
use http::Response;
use ngsi_ld::{
    client::{ReqwestError, ReqwestTransport, Transport},
    http::UncheckedRequest,
};
use reqwest::redirect::Policy;
use std::time::Duration;

/// The longest a request may take, as for every test client.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a request could not be sent or answered.
#[derive(Debug, thiserror::Error)]
pub enum RawAnswerError {
    /// The HTTP client could not be built.
    #[error("the HTTP client could not be built")]
    Http(#[source] reqwest::Error),
    /// The request could not be sent or the answer not received.
    #[error("the request failed")]
    Transport(#[source] ReqwestError),
}

/// Sends `request` and returns the response whatever its status, with its body byte for byte as received; the exchange
/// is printed as every test client prints its own.
///
/// # Errors
///
/// Returns [`RawAnswerError`] when the HTTP client cannot be built or no response is received.
pub async fn send_raw(request: UncheckedRequest) -> Result<Response<Bytes>, RawAnswerError> {
    let http = reqwest::Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(RawAnswerError::Http)?;
    ExchangeLog::new(ReqwestTransport::new(http))
        .send(request.into_request())
        .await
        .map_err(RawAnswerError::Transport)
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, raw_answer::send_raw, wire::request_to};
    use googletest::prelude::*;
    use http::{Method, StatusCode};
    use url::Url;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

    #[gtest]
    #[tokio::test]
    async fn the_body_of_a_406_answer_is_kept_whole() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(406).set_body_string(r#"{"availableRepresentations": ["application/json"]}"#))
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let response = send_raw(request_to(&client, Method::GET, "types")?).await?;
        expect_that!(response.status(), eq(StatusCode::NOT_ACCEPTABLE));
        verify_that!(
            String::from_utf8_lossy(response.body()).as_ref(),
            eq(r#"{"availableRepresentations": ["application/json"]}"#)
        )
    }
}
