//! The URL a broker mints for a context stored by Add @context (clause 6.29.3.1) when the request carries the
//! `Forwarded` (IETF RFC 7239) or `X-Forwarded-Proto` fields: the `Location` it answers, and the `URL` its metadata
//! reports (clause 5.13.3.5), which a broker believing its edge builds from those fields.
//!
//! The typed Add @context writes none of those fields, so the request is the crate's own operation sent unchecked
//! with the fields appended, and the metadata is read from the stored context's path as the `Location` wrote it.

use crate::{
    broker::BrokerClient,
    context_details::{self, ContextDetailsError},
    namespace::Namespace,
    request_fields::{FieldError, with_fields},
    wire::{WireError, content_type, request_to},
};
use http::{Method, StatusCode, Uri, header::LOCATION, uri::InvalidUri};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::operation::AddContext,
    model::{APPLICATION_JSON, Context, ContextEntry, InlineContext, InlineContextError},
};
use url::Url;

/// Why a context could not be minted, read back or removed.
#[derive(Debug, thiserror::Error)]
pub enum MintError {
    /// The context to store is not valid.
    #[error("the context to store is not valid")]
    Context(#[source] InlineContextError),
    /// A header field cannot be written.
    #[error(transparent)]
    Field(#[from] FieldError),
    /// The request could not be built or sent.
    #[error("the request failed")]
    Client(#[source] Box<ClientError<ReqwestError>>),
    /// The request target could not be built.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The broker answered another status than the one required.
    #[error("the broker answered {actual} where {expected} was required")]
    Status {
        /// The status required.
        expected: StatusCode,
        /// The status the broker answered.
        actual: StatusCode,
    },
    /// The answer carries no `Location` that reads as text.
    #[error("the answer names no Location")]
    NoLocation,
    /// The `Location` does not resolve against the broker's API root.
    #[error("the Location does not resolve against {0}")]
    Resolve(Url, #[source] url::ParseError),
    /// The resolved `Location`, with its query, is not a request target.
    #[error("{0} with its query is not a request target")]
    Target(Url, #[source] InvalidUri),
    /// The metadata of the stored context does not read.
    #[error(transparent)]
    Details(#[from] ContextDetailsError),
}

/// A context stored through a request that carried forwarded fields.
#[derive(Debug, Clone)]
pub struct MintedContext {
    /// The `Location` the broker answered, as written.
    pub location: String,
    /// The `URL` the metadata of the stored context reports.
    pub url: Url,
}

impl MintedContext {
    /// The URL `location` has on the server at `origin`: what a client reaching the broker there is told.
    ///
    /// # Errors
    ///
    /// Returns [`url::ParseError`] when the location does not resolve against `origin`.
    pub fn on(&self, origin: &Url) -> Result<Url, url::ParseError> {
        origin.join(&self.location)
    }
}

/// The request target of the path `location` on the server `client` speaks to, with `query` appended.
fn target(client: &BrokerClient, location: &str, query: &str) -> Result<Uri, MintError> {
    let base = client.endpoint().base_url();
    // The error owns the base URL it names, borrowed from the client.
    let resolved = base.join(location).map_err(|error| MintError::Resolve(base.clone(), error))?;
    format!("{resolved}{query}").parse().map_err(|error| MintError::Target(resolved, error))
}

/// Stores a context holding the term `<stem>_<namespace>`, defined as `https://example.org/<namespace>/<stem>`, through
/// `client`, the request carrying `headers` as given, answered 201, and reads the `URL` its metadata reports.
///
/// # Errors
///
/// Returns [`MintError`] when a field is not valid, the broker answers another status than 201 to either request or
/// names no `Location`, or the metadata does not read.
pub async fn mint(client: &BrokerClient, namespace: &Namespace, stem: &str, headers: &[(&str, &str)]) -> Result<MintedContext, MintError> {
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"{stem}_{namespace}": "https://example.org/{namespace}/{stem}"}}"#)
            .parse::<InlineContext>()
            .map_err(MintError::Context)?,
    ));
    let request = content_type(
        client
            .unchecked(&AddContext::builder().context(&context).build())
            .map_err(|error| MintError::Client(Box::new(error)))?,
        &APPLICATION_JSON,
    )
    .map_err(WireError::MediaType)?;
    let response = client
        .send_unchecked(with_fields(request, headers)?)
        .await
        .map_err(|error| MintError::Client(Box::new(error)))?;
    require(response.status(), StatusCode::CREATED)?;
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(MintError::NoLocation)?
        .to_owned();
    let details = client
        .send_unchecked(request_to(client, Method::GET, "")?.uri(target(client, &location, "?details=true")?))
        .await
        .map_err(|error| MintError::Client(Box::new(error)))?;
    require(details.status(), StatusCode::OK)?;
    let url = context_details::metadata(&details)?.url;
    Ok(MintedContext { location, url })
}

/// Removes the context `minted` names, answered 204.
///
/// # Errors
///
/// Returns [`MintError`] when the request fails or the broker answers another status than 204.
pub async fn remove(client: &BrokerClient, minted: &MintedContext) -> Result<(), MintError> {
    let response = client
        .send_unchecked(request_to(client, Method::DELETE, "")?.uri(target(client, &minted.location, "")?))
        .await
        .map_err(|error| MintError::Client(Box::new(error)))?;
    require(response.status(), StatusCode::NO_CONTENT)
}

/// `Ok` when `actual` is `expected`.
const fn require(actual: StatusCode, expected: StatusCode) -> Result<(), MintError> {
    if actual.as_u16() == expected.as_u16() {
        Ok(())
    } else {
        Err(MintError::Status { expected, actual })
    }
}

#[cfg(test)]
mod tests {
    use crate::minted_context_url::{MintedContext, require};
    use googletest::prelude::*;
    use http::StatusCode;
    use url::Url;

    #[gtest]
    fn a_location_has_the_url_of_the_server_that_serves_it() -> Result<()> {
        let minted = MintedContext {
            location: "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a1".to_owned(),
            url: Url::parse("http://broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a1")?,
        };
        let origin = Url::parse("https://edge.example:8443/")?;
        verify_that!(
            minted.on(&origin)?.as_str(),
            eq("https://edge.example:8443/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a1")
        )
    }

    #[gtest]
    fn another_status_than_the_required_one_is_refused() {
        expect_that!(require(StatusCode::CREATED, StatusCode::CREATED).is_ok(), eq(true));
        expect_that!(require(StatusCode::OK, StatusCode::CREATED).is_err(), eq(true));
    }
}
