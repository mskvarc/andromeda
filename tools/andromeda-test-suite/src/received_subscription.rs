//! Retrieve Subscription and Query Subscriptions answered as received, for a Subscription whose `q` names a prefixed
//! name or an absolute IRI.
//!
//! The client's `q` model reads a name only as an unquoted `AttrName`, so a Subscription whose `q` holds a quoted name
//! does not read through the model at all. The request is the typed operation's, and the answer comes back as
//! received, to be read with the matchers of [`crate::body_json`]. A reader whose `q` reads as `AttrName`s retrieves the
//! Subscription typed.

use crate::broker::BrokerClient;
use bytes::Bytes;
use http::Response;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{
        headers::Accept,
        operation::{QuerySubscriptions, RetrieveSubscription},
    },
    model::NgsiLdMediaType,
    subscription::{EntitySubscription, SubscriptionId},
};

/// Why a Subscription could not be read as received.
#[derive(Debug, thiserror::Error)]
#[error("the subscription request could not be built, sent or answered")]
pub struct ReceivedSubscriptionError(#[source] Box<ClientError<ReqwestError>>);

/// The client's error, boxed, since it is too large to hold in the error unboxed.
fn client_error(error: ClientError<ReqwestError>) -> ReceivedSubscriptionError {
    ReceivedSubscriptionError(Box::new(error))
}

/// The answer to Retrieve Subscription `id` by the broker `client` speaks to, as `application/json` under the
/// `@context` the client names.
///
/// # Errors
///
/// Returns [`ReceivedSubscriptionError`] when the request cannot be built or sent, or the answer is an error.
pub async fn retrieved_as_received(client: &BrokerClient, id: &SubscriptionId) -> Result<Response<Bytes>, ReceivedSubscriptionError> {
    let client = client.with_accept(Accept::new(NgsiLdMediaType::Json));
    let request = client
        .unchecked(&RetrieveSubscription::builder().id(id).kind(EntitySubscription).build())
        .map_err(client_error)?;
    client.send_unchecked(request).await.map_err(client_error)
}

/// The answer to Query Subscriptions by the broker `client` speaks to, as `application/json` under the `@context` the
/// client names.
///
/// # Errors
///
/// Returns [`ReceivedSubscriptionError`] when the request cannot be built or sent, or the answer is an error.
pub async fn listed_as_received(client: &BrokerClient) -> Result<Response<Bytes>, ReceivedSubscriptionError> {
    let client = client.with_accept(Accept::new(NgsiLdMediaType::Json));
    let request = client
        .unchecked(&QuerySubscriptions::builder().kind(EntitySubscription).build())
        .map_err(client_error)?;
    client.send_unchecked(request).await.map_err(client_error)
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::client_at,
        received_subscription::{listed_as_received, retrieved_as_received},
    };
    use googletest::prelude::*;
    use http::StatusCode;
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{header, method, path},
    };

    /// A Subscription whose `q` the model cannot read.
    const QUOTED: &str = r#"{"id": "urn:ngsi-ld:Subscription:1", "type": "Subscription", "q": "\"urn:x:a;b\">50"}"#;

    #[gtest]
    #[tokio::test]
    async fn a_subscription_the_model_cannot_read_comes_back_as_received() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:1"))
            .and(header("accept", "application/json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(QUOTED),
            )
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let answer = retrieved_as_received(&client, &"urn:ngsi-ld:Subscription:1".parse()?).await?;
        expect_that!(answer.status(), eq(StatusCode::OK));
        verify_that!(String::from_utf8_lossy(answer.body()).into_owned(), contains_substring("urn:x:a;b"))
    }

    #[gtest]
    #[tokio::test]
    async fn a_listing_the_model_cannot_read_comes_back_as_received() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ngsi-ld/v1/subscriptions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(format!("[{QUOTED}]")),
            )
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let answer = listed_as_received(&client).await?;
        verify_that!(answer.status(), eq(StatusCode::OK))
    }
}
