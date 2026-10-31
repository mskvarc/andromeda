//! Bodies a broker must refuse as bad request data (clause 6.3.2), each posted as the test writes it: a body the typed
//! model cannot write because it breaks the definition the model enforces.

use crate::{broker::BrokerClient, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Sends each of `bodies` with `method` to `path` (relative to the API root) under `Content-Type: application/json`
/// and expects 400 `BadRequestData` for every one; a body the broker accepts fails the test.
///
/// # Errors
///
/// Returns the failure when a request cannot be built.
pub async fn expect_bad_request_data(broker: &BrokerClient, method: &Method, path: &str, bodies: &[String]) -> Result<()> {
    for body in bodies {
        let request = json_request_to(broker, method.clone(), path, body)?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, refused_body::expect_bad_request_data};
    use googletest::prelude::*;
    use http::Method;
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{body_string, method, path},
    };

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn every_body_is_sent_as_written_and_answered_with_bad_request_data() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ngsi-ld/v1/csourceRegistrations"))
            .and(body_string("{ \"type\":  1 }"))
            .respond_with(ResponseTemplate::new(400).set_body_raw(
                r#"{"type":"https://uri.etsi.org/ngsi-ld/errors/BadRequestData","title":"Bad request data","detail":"refused"}"#,
                "application/json",
            ))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        expect_bad_request_data(&client, &Method::POST, "csourceRegistrations", &["{ \"type\":  1 }".to_owned()]).await
    }
}
