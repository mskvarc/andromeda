//! An operation sent under a `Prefer: ngsi-ld=<version>` preference and the answer kept as received, for a test
//! whose subject is the wire form of a payload adapted to an older version (clause 4.3.6.8), which the current model
//! has no place for.

use crate::{broker::BrokerClient, prefer_text::preference_applied};
use bytes::Bytes;
use http::StatusCode;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{
        headers::{Prefer, PreferError, SpecVersion, SpecVersionError},
        operation::Operation,
    },
};

/// Why an operation could not be sent under a version.
#[derive(Debug, thiserror::Error)]
pub enum VersionedError {
    /// The version is not a specification version.
    #[error(transparent)]
    Version(#[from] SpecVersionError),
    /// The preference cannot be written.
    #[error(transparent)]
    Prefer(#[from] PreferError),
    /// The request could not be built or the broker refused it.
    #[error("the request failed")]
    Client(#[source] Box<ClientError<ReqwestError>>),
}

/// The answer to an operation sent under a version, as received.
#[derive(Debug, Clone)]
pub struct VersionedAnswer {
    /// The status of the answer.
    pub status: StatusCode,
    /// The value of the `Preference-Applied` header, when exactly one field line carries it.
    pub preference_applied: Option<String>,
    /// The body, byte for byte.
    pub body: Bytes,
}

/// Sends `operation` through `client` with `Prefer: ngsi-ld=<version>`, or with no preference when `version` is `None`,
/// and keeps the answer as received.
///
/// # Errors
///
/// Returns [`VersionedError`] when the version is invalid, the request cannot be built or the broker answers an
/// error status.
pub async fn send_versioned<O: Operation + Sync>(client: &BrokerClient, operation: &O, version: Option<&str>) -> Result<VersionedAnswer, VersionedError> {
    let client = match version {
        Some(version) => client.with_prefer(Prefer::builder().version(version.parse::<SpecVersion>()?).build()?),
        None => client.without_prefer(),
    };
    let request = client.unchecked(operation).map_err(|error| VersionedError::Client(Box::new(error)))?;
    let response = client.send_unchecked(request).await.map_err(|error| VersionedError::Client(Box::new(error)))?;
    Ok(VersionedAnswer {
        status: response.status(),
        preference_applied: preference_applied(response.headers()).map(str::to_owned),
        body: response.into_body(),
    })
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, versioned_answer::send_versioned};
    use googletest::prelude::*;
    use http::StatusCode;
    use ngsi_ld::http::operation::RetrieveEntityTypes;
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{header, method},
    };

    #[gtest]
    #[tokio::test]
    async fn the_answer_to_an_operation_sent_under_a_version_is_kept_as_received() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("prefer", "ngsi-ld=1.0"))
            .respond_with(
                ResponseTemplate::new(203)
                    .insert_header("preference-applied", "ngsi-ld=1.0")
                    .set_body_string(r#"{"typeList": []}"#),
            )
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let answer = send_versioned(&client, &RetrieveEntityTypes::builder().build(), Some("1.0")).await?;
        expect_that!(answer.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
        expect_that!(answer.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
        verify_that!(String::from_utf8_lossy(&answer.body).as_ref(), eq(r#"{"typeList": []}"#))
    }

    #[gtest]
    #[tokio::test]
    async fn a_version_that_is_no_version_is_refused_before_sending() -> Result<()> {
        let client = client_at(Url::parse("http://127.0.0.1:9/ngsi-ld/v1/")?)?;
        verify_that!(
            send_versioned(&client, &RetrieveEntityTypes::builder().build(), Some("latest")).await,
            err(anything())
        )
    }
}
