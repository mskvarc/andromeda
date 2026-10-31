//! The `@context` a broker links for a request context no single URL names (clause 6.3.6): the wrapper it hosts as an
//! `ImplicitlyCreated` context (clause 5.13.1) and names by an absolute URL of its own minting.

use crate::{
    broker::BrokerClient,
    hosted_context::{HostedContext, HostedContextError},
    wire::header_values,
};
use http::header::LINK;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{
        ResponseHead,
        headers::{Accept, Link},
    },
    model::{ContextId, DocumentError, LocalContext},
};
use regex::Regex;
use std::{slice, str, str::Utf8Error};
use url::Url;

/// The text every minted wrapper URL holds, which a listing is searched for.
const WRAPPER_MARKER: &str = "JsonLdContext";

/// What a minted wrapper URL looks like: an absolute URL on the API root, ending with the identifier of an
/// `ImplicitlyCreated` context, with no query.
const MINTED_WRAPPER: &str = r"^https?://[^?]+/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:[^?/]+$";

/// Why a link or a URL is not a minted wrapper, or the wrapper cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum WrapperError {
    /// The answer carries its `Link` on no field line or on several.
    #[error("the answer carries the Link header on {0} field lines, not one")]
    LinkLines(usize),
    /// The `Link` header does not read.
    #[error("the Link header does not read")]
    LinkUnreadable,
    /// The `Link` header names no JSON-LD context or several.
    #[error("the Link header names {0} JSON-LD contexts, not one")]
    ContextLinks(usize),
    /// The context link target is not an absolute URL.
    #[error("the context link {0} is not an absolute URL")]
    NotAbsolute(IriRefBuf, #[source] url::ParseError),
    /// The URL is not one a broker mints for a wrapper.
    #[error("{0} is not a minted wrapper URL")]
    NotMinted(Url),
    /// The shape check could not be compiled.
    #[error("the wrapper shape is not a regular expression")]
    Shape(#[source] regex::Error),
    /// The URL names no stored context.
    #[error(transparent)]
    Hosted(#[from] HostedContextError),
    /// The broker refused to serve the wrapper.
    #[error("the broker did not serve the wrapper")]
    Serve(#[source] Box<ClientError<ReqwestError>>),
    /// The served document is not UTF-8.
    #[error("the served wrapper is not UTF-8")]
    Utf8(#[from] Utf8Error),
    /// The served document is not a local context document.
    #[error(transparent)]
    Document(#[from] DocumentError),
}

/// The one JSON-LD context link of `head`: the `Link` header on exactly one field line naming exactly one context.
///
/// # Errors
///
/// Returns [`WrapperError`] when the header is absent, unreadable, split over lines, or names no context or several.
pub fn linked_context(head: &ResponseHead) -> Result<Url, WrapperError> {
    let lines = header_values(head.headers(), &LINK).len();
    if lines != 1 {
        return Err(WrapperError::LinkLines(lines));
    }
    let link = head.typed::<Link>().into_valid().ok_or(WrapperError::LinkUnreadable)?;
    let mut targets = link.context_urls();
    let (Some(target), None) = (targets.next(), targets.next()) else {
        return Err(WrapperError::ContextLinks(link.context_urls().count()));
    };
    // The error owns the target it names, borrowed from the header.
    Url::parse(target.as_str()).map_err(|error| WrapperError::NotAbsolute(target.clone(), error))
}

/// The wrapper `url` names, when it has the shape of a URL a broker mints for a hosted wrapper.
///
/// # Errors
///
/// Returns [`WrapperError::NotMinted`] when `url` has another shape, and [`WrapperError::Hosted`] when its last
/// segment is no context identifier.
pub fn minted_at(url: &Url) -> Result<HostedContext, WrapperError> {
    let shape = Regex::new(MINTED_WRAPPER).map_err(WrapperError::Shape)?;
    if !shape.is_match(url.as_str()) {
        return Err(WrapperError::NotMinted(url.clone()));
    }
    Ok(HostedContext::listed(slice::from_ref(url), "JsonLdContext")?)
}

/// The wrapper the one context link of `head` names, which must have the shape of a URL a broker mints.
///
/// # Errors
///
/// Returns [`WrapperError`] when [`linked_context`] or [`minted_at`] refuses it.
pub fn minted_wrapper(head: &ResponseHead) -> Result<HostedContext, WrapperError> {
    minted_at(&linked_context(head)?)
}

/// The identifiers of the wrappers a listing of stored contexts names, in the order listed: every listed URL ending
/// with the identifier of an `ImplicitlyCreated` context.
///
/// # Errors
///
/// Returns [`WrapperError::Hosted`] when a listed URL holding the marker names no stored context.
pub fn listed_wrapper_ids(listed: &[Url]) -> Result<Vec<ContextId>, WrapperError> {
    listed
        .iter()
        .filter(|url| url.as_str().contains(WRAPPER_MARKER))
        .map(|url| Ok(HostedContext::listed(slice::from_ref(url), WRAPPER_MARKER)?.id))
        .collect()
}

/// The document `client` is served for `wrapper`, read whole: the `@context` and any member beside it.
///
/// # Errors
///
/// Returns [`WrapperError::Serve`] when the broker refuses, and the read errors of the document otherwise.
pub async fn served_document(client: &BrokerClient, wrapper: &HostedContext) -> Result<LocalContext, WrapperError> {
    let received = client
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&wrapper.id)
        .send()
        .await
        .map_err(|error| WrapperError::Serve(Box::new(error)))?;
    Ok(LocalContext::parse(str::from_utf8(&received.body)?)?)
}

#[cfg(test)]
mod tests {
    use crate::minted_wrapper::{linked_context, listed_wrapper_ids, minted_at, minted_wrapper};
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderValue, header::LINK};
    use ngsi_ld::http::ResponseHead;
    use url::Url;

    /// A head whose `Link` field lines are `lines`.
    fn head_with(lines: &[&'static str]) -> ResponseHead {
        let mut headers = HeaderMap::new();
        for line in lines {
            headers.append(LINK, HeaderValue::from_static(line));
        }
        ResponseHead::new(headers)
    }

    const MINTED: &str = r#"<http://broker:1026/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:abc>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;

    #[gtest]
    fn the_one_context_link_of_a_head_is_its_target() -> Result<()> {
        let url = linked_context(&head_with(&[MINTED]))?;
        verify_that!(url.as_str(), eq("http://broker:1026/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:abc"))
    }

    #[gtest]
    fn a_head_with_no_link_or_two_context_links_has_no_one_context() {
        expect_that!(linked_context(&head_with(&[])).is_err(), eq(true));
        expect_that!(linked_context(&head_with(&[MINTED, MINTED])).is_err(), eq(true));
        expect_that!(
            linked_context(&head_with(&[
                r#"<http://a.test/x>; rel="http://www.w3.org/ns/json-ld#context", <http://a.test/y>; rel="http://www.w3.org/ns/json-ld#context""#
            ]))
            .is_err(),
            eq(true)
        );
    }

    #[gtest]
    fn a_minted_wrapper_names_the_context_its_url_ends_with() -> Result<()> {
        let wrapper = minted_wrapper(&head_with(&[MINTED]))?;
        verify_that!(wrapper.id.to_string(), eq("urn:ngsi-ld:JsonLdContext:abc"))
    }

    #[gtest]
    fn a_url_of_another_shape_is_no_minted_wrapper() -> Result<()> {
        expect_that!(minted_at(&Url::parse("http://mock.test/jc/terms.jsonld")?).is_err(), eq(true));
        expect_that!(
            minted_at(&Url::parse("http://broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a?x=1")?).is_err(),
            eq(true)
        );
        verify_that!(
            minted_wrapper(&head_with(&[
                r#"<http://mock.test/jc/terms.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#
            ]))
            .is_err(),
            eq(true)
        )
    }

    #[gtest]
    fn a_listing_names_the_wrappers_among_its_urls_in_order() -> Result<()> {
        let listed = [
            Url::parse("http://broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:context:hosted")?,
            Url::parse("http://broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a")?,
            Url::parse("http://broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:b")?,
        ];
        let ids = listed_wrapper_ids(&listed)?;
        expect_that!(
            ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            elements_are![eq("urn:ngsi-ld:JsonLdContext:a"), eq("urn:ngsi-ld:JsonLdContext:b")]
        );
        verify_that!(listed_wrapper_ids(&listed[..1])?, is_empty())
    }
}
