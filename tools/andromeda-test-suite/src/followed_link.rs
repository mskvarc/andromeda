//! The request a test sends to the target of a `Link` a broker answered with (clause 6.3.10): the target is resolved
//! against the broker's address, whatever path it names, and the request carries the client's tenant and headers.

use crate::{
    broker::BrokerClient,
    wire::{WireError, request_to},
};
use http::{Method, Uri, uri::InvalidUri};
use iri_rs::IriRefBuf;
use ngsi_ld::http::{
    UncheckedRequest,
    headers::{Link, RelationType},
};
use url::Url;

/// Why a link could not be followed.
#[derive(Debug, thiserror::Error)]
pub enum FollowError {
    /// The answer carries no link with the relation.
    #[error("the answer carries no link with the relation {0}")]
    NoLink(RelationType),
    /// The target does not resolve against the broker's address.
    #[error("the link target {0} does not resolve")]
    Target(IriRefBuf, #[source] url::ParseError),
    /// The resolved target is not a request target.
    #[error("the resolved link target is not a URI")]
    Uri(#[source] InvalidUri),
    /// The request could not be started.
    #[error(transparent)]
    Wire(#[from] WireError),
}

/// The target of the first link of `link` with the relation `relation`, resolved against the API root of `client`.
fn resolved_target(client: &BrokerClient, link: &Link, relation: &RelationType) -> Result<Url, FollowError> {
    let target = link.find_by_rel(relation).next().ok_or_else(|| FollowError::NoLink(relation.clone()))?.target();
    client
        .endpoint()
        .base_url()
        .join(target.as_str())
        // The error owns the target it names; the link keeps its own.
        .map_err(|error| FollowError::Target(target.clone(), error))
}

/// A GET request to the target of the first link of `link` with the relation `relation`, resolved against the API
/// root of `client`.
///
/// # Errors
///
/// Returns [`FollowError`] when the header has no such link or its target does not resolve.
pub fn request(client: &BrokerClient, link: &Link, relation: &RelationType) -> Result<UncheckedRequest, FollowError> {
    let uri: Uri = resolved_target(client, link, relation)?.as_str().parse().map_err(FollowError::Uri)?;
    Ok(request_to(client, Method::GET, "")?.uri(uri))
}

/// A GET request to the target of the first link of `link` with the relation `relation`, resolved against the API
/// root of `client`, with `parameters` appended to its query as written (their names and values are not encoded).
///
/// # Errors
///
/// Returns [`FollowError`] when the header has no such link or its target does not resolve.
pub fn request_with(client: &BrokerClient, link: &Link, relation: &RelationType, parameters: &[(&str, &str)]) -> Result<UncheckedRequest, FollowError> {
    let resolved = resolved_target(client, link, relation)?;
    // The link's own query keeps the bytes the broker wrote; a parameter is appended after it as written.
    let query = parameters.iter().enumerate().fold(String::new(), |mut query, (position, (name, value))| {
        let separator = if position == 0 && resolved.query().is_none() { '?' } else { '&' };
        query.push(separator);
        query.push_str(name);
        query.push('=');
        query.push_str(value);
        query
    });
    let uri: Uri = format!("{resolved}{query}").parse().map_err(FollowError::Uri)?;
    Ok(request_to(client, Method::GET, "")?.uri(uri))
}

/// A GET request to the target of the first link of `link` with the relation `relation`, resolved against the API
/// root of `client`, with its query parameter `name` set to `value` (or dropped when `value` is `None`) and every
/// other parameter kept in order; the query is written again with each name and value form-encoded.
fn request_changing(client: &BrokerClient, link: &Link, relation: &RelationType, name: &str, value: Option<&str>) -> Result<UncheckedRequest, FollowError> {
    let mut resolved = resolved_target(client, link, relation)?;
    let pairs = resolved
        .query_pairs()
        .filter_map(|(key, kept)| match (key == name, value) {
            (true, Some(replacement)) => Some((key.into_owned(), replacement.to_owned())),
            (true, None) => None,
            (false, Some(_) | None) => Some((key.into_owned(), kept.into_owned())),
        })
        .collect::<Vec<_>>();
    resolved.query_pairs_mut().clear().extend_pairs(pairs);
    let uri: Uri = resolved.as_str().parse().map_err(FollowError::Uri)?;
    Ok(request_to(client, Method::GET, "")?.uri(uri))
}

/// A GET request to the target of the first link of `link` with the relation `relation`, resolved against the API
/// root of `client`, with its query parameter `name` set to `value` and every other parameter kept in order.
///
/// # Errors
///
/// Returns [`FollowError`] when the header has no such link or its target does not resolve.
pub fn request_replacing(client: &BrokerClient, link: &Link, relation: &RelationType, name: &str, value: &str) -> Result<UncheckedRequest, FollowError> {
    request_changing(client, link, relation, name, Some(value))
}

/// A GET request to the target of the first link of `link` with the relation `relation`, resolved against the API
/// root of `client`, without its query parameter `name` and with every other parameter kept in order.
///
/// # Errors
///
/// Returns [`FollowError`] when the header has no such link or its target does not resolve.
pub fn request_without(client: &BrokerClient, link: &Link, relation: &RelationType, name: &str) -> Result<UncheckedRequest, FollowError> {
    request_changing(client, link, relation, name, None)
}

#[cfg(test)]
mod tests {
    use crate::{
        broker,
        followed_link::{request, request_replacing, request_with, request_without},
    };
    use googletest::prelude::*;
    use iri_rs::IriRefBuf;
    use ngsi_ld::http::headers::{Link, LinkValue, RelationType};

    #[gtest]
    fn the_target_of_the_next_link_resolves_against_the_broker() -> Result<()> {
        let client = broker::client_at("http://broker.test:1026/ngsi-ld/v1/".parse()?)?;
        let link = Link::try_new(
            LinkValue::builder("/ngsi-ld/v1/temporal/entities/urn:x:1?timeAt=2020-01-01T01%3A09%3A00.000001Z".parse::<IriRefBuf>()?)
                .rel(RelationType::Next)
                .build(),
        )?;
        let followed = request(&client, &link, &RelationType::Next)?;
        verify_that!(
            followed.as_request().uri().to_string(),
            eq("http://broker.test:1026/ngsi-ld/v1/temporal/entities/urn:x:1?timeAt=2020-01-01T01%3A09%3A00.000001Z")
        )
    }

    #[gtest]
    fn a_header_without_the_relation_cannot_be_followed() -> Result<()> {
        let client = broker::client_at("http://broker.test:1026/ngsi-ld/v1/".parse()?)?;
        let link = Link::try_json_ld_context("http://ctx.test/c.jsonld".parse::<IriRefBuf>()?)?;
        verify_that!(request(&client, &link, &RelationType::Next).is_err(), eq(true))
    }

    #[gtest]
    fn the_parameters_follow_the_query_the_link_carries() -> Result<()> {
        let client = broker::client_at("http://broker.test:1026/ngsi-ld/v1/".parse()?)?;
        let link = Link::try_new(
            LinkValue::builder("/ngsi-ld/v1/entities?type=T&limit=100&cursor=a".parse::<IriRefBuf>()?)
                .rel(RelationType::Next)
                .build(),
        )?;
        let followed = request_with(&client, &link, &RelationType::Next, &[("count", "true")])?;
        verify_that!(
            followed.as_request().uri().to_string(),
            eq("http://broker.test:1026/ngsi-ld/v1/entities?type=T&limit=100&cursor=a&count=true")
        )
    }

    #[gtest]
    fn a_replaced_parameter_keeps_its_place_among_the_others() -> Result<()> {
        let client = broker::client_at("http://broker.test:1026/ngsi-ld/v1/".parse()?)?;
        let link = Link::try_new(
            LinkValue::builder("/ngsi-ld/v1/entities?type=T&q=height%3E5&cursor=a".parse::<IriRefBuf>()?)
                .rel(RelationType::Next)
                .build(),
        )?;
        let followed = request_replacing(&client, &link, &RelationType::Next, "q", "height>25")?;
        verify_that!(
            followed.as_request().uri().to_string(),
            eq("http://broker.test:1026/ngsi-ld/v1/entities?type=T&q=height%3E25&cursor=a")
        )
    }

    #[gtest]
    fn a_dropped_parameter_leaves_the_others_in_order() -> Result<()> {
        let client = broker::client_at("http://broker.test:1026/ngsi-ld/v1/".parse()?)?;
        let link = Link::try_new(
            LinkValue::builder("/ngsi-ld/v1/entities?type=T&cursor=a&pageQuery=b&limit=2".parse::<IriRefBuf>()?)
                .rel(RelationType::Next)
                .build(),
        )?;
        let followed = request_without(&client, &link, &RelationType::Next, "pageQuery")?;
        verify_that!(
            followed.as_request().uri().to_string(),
            eq("http://broker.test:1026/ngsi-ld/v1/entities?type=T&cursor=a&limit=2")
        )
    }
}
