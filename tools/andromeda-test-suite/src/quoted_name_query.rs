//! Query Entities and Retrieve Subscription with a `q` naming a prefixed name or an absolute IRI, for a test whose
//! subject is how a `q` spells a name no term of the `@context` gives.
//!
//! The client's `q` model reads and writes a name only as an unquoted `AttrName`, so it can neither build nor read a
//! `q` holding a quoted name. The request is sent with the `q` as written, under the `@context` a `Link` header names
//! when one is given, and the answer comes back as received, to be read with [`crate::unchecked_page::entities`] and
//! the matchers of [`crate::body_json`].

use crate::{
    broker::BrokerClient,
    hosted_context::HostedContext,
    raw_answer::{RawAnswerError, send_raw},
    raw_call::target,
    wire::{WireError, request_to},
};
use bytes::Bytes;
use http::{HeaderValue, Method, Response, header::ACCEPT};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::{Link, LinkBuildError},
    model::EntityTypeName,
    subscription::SubscriptionId,
};

/// Why a request with a written `q` could not be sent or answered.
#[derive(Debug, thiserror::Error)]
pub enum QuotedNameQueryError {
    /// The request could not be written.
    #[error("the request could not be written")]
    Request(#[from] WireError),
    /// The request could not be sent or answered.
    #[error("the request was not answered")]
    Answer(#[from] RawAnswerError),
    /// The URL a stored `@context` is served at is no IRI reference.
    #[error("the URL of a stored @context is no IRI reference")]
    NotAnIri,
    /// The `Link` header naming a stored `@context` cannot be built.
    #[error("the Link header naming a stored @context cannot be built")]
    Link(#[from] LinkBuildError),
}

/// The `Link` header naming the `@context` `hosted` as the one a request is read under (clause 6.3.5).
///
/// # Errors
///
/// Returns [`QuotedNameQueryError`] when the URL is no IRI reference or cannot be carried in the header.
pub fn context_link(hosted: &HostedContext) -> Result<Link, QuotedNameQueryError> {
    let iri = IriRefBuf::new(hosted.url.to_string()).map_err(|_| QuotedNameQueryError::NotAnIri)?;
    Ok(Link::try_json_ld_context(iri)?)
}

/// Sends `path` (relative to the API root) with `parameters` as written by GET as `application/json`, under the
/// `@context` `link` names when one is given.
///
/// # Errors
///
/// Returns [`QuotedNameQueryError`] when the request cannot be written, sent or answered.
pub async fn get_as_written(
    client: &BrokerClient,
    path: &str,
    parameters: &[(&str, &str)],
    link: Option<&Link>,
) -> Result<Response<Bytes>, QuotedNameQueryError> {
    let request = request_to(client, Method::GET, &target(path, parameters))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let request = match link {
        Some(link) => request.typed_header(link.clone()),
        None => request,
    };
    Ok(send_raw(request).await?)
}

/// Query Entities for the Entities of `entity_type` with the `q` `q` as written and `parameters` beside it, under the
/// `@context` `link` names when one is given.
///
/// # Errors
///
/// Returns [`QuotedNameQueryError`] when the request cannot be written, sent or answered.
pub async fn query_by_text(
    client: &BrokerClient,
    entity_type: &EntityTypeName,
    q: &str,
    parameters: &[(&str, &str)],
    link: Option<&Link>,
) -> Result<Response<Bytes>, QuotedNameQueryError> {
    let entity_type = entity_type.to_string();
    let mut all = vec![("type", entity_type.as_str()), ("q", q)];
    all.extend_from_slice(parameters);
    get_as_written(client, "entities", &all, link).await
}

/// Retrieve Subscription `id`, under the `@context` `link` names when one is given.
///
/// # Errors
///
/// Returns [`QuotedNameQueryError`] when the request cannot be written, sent or answered.
pub async fn retrieve_by_text(client: &BrokerClient, id: &SubscriptionId, link: Option<&Link>) -> Result<Response<Bytes>, QuotedNameQueryError> {
    get_as_written(client, &format!("subscriptions/{id}"), &[], link).await
}
