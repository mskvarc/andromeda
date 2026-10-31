//! A page of entities read from a query the typed client cannot write (a `cursor` parameter, a language range the
//! model refuses, a query string sent as written): the entities of the answer and the pages its links lead to (clause
//! 6.3.10).

use crate::{
    broker::BrokerClient,
    followed_link::{FollowError, request},
    link_header::{LinkUnreadable, link},
};
use bytes::Bytes;
use http::{HeaderValue, Response, header::ACCEPT};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::headers::RelationType,
    model::{DocumentError, Entity, Lossless, Output},
};
use serde_json::Value;

/// What a broker answered to a request sent unchecked.
pub type Answer = Result<Response<Bytes>, ClientError<ReqwestError>>;

/// Why an answer is not a page of entities, or a linked page cannot be requested.
#[derive(Debug, thiserror::Error)]
pub enum UncheckedPageError {
    /// The body is not JSON.
    #[error("the body is not JSON")]
    NotJson(#[source] serde_json::Error),
    /// The body is not an array.
    #[error("the body is not an array of entities")]
    NotAnArray,
    /// An item of the body is not an entity.
    #[error("an item of the page is not an entity")]
    Entity(#[source] DocumentError),
    /// The `Link` header does not read.
    #[error(transparent)]
    Link(#[from] LinkUnreadable),
    /// The link cannot be requested.
    #[error(transparent)]
    Follow(#[from] FollowError),
}

/// The entities of the body of `response`, an array of entities.
///
/// # Errors
///
/// Returns [`UncheckedPageError`] when the body is not an array of entities.
pub fn entities(response: &Response<Bytes>) -> Result<Vec<Entity<Output>>, UncheckedPageError> {
    let document: Value = serde_json::from_slice(response.body()).map_err(UncheckedPageError::NotJson)?;
    let Value::Array(items) = document else {
        return Err(UncheckedPageError::NotAnArray);
    };
    items
        .iter()
        .map(|item| Entity::<Output>::parse(&item.to_string(), Lossless::Normalized).map_err(UncheckedPageError::Entity))
        .collect()
}

/// The page the link of relation `relation` (`first`, `prev`, `next` or `last`) of `response` leads to, requested with
/// `Accept: application/json`; `None` when the answer carries no such link.
///
/// # Errors
///
/// Returns [`UncheckedPageError`] when the link does not read or does not resolve.
pub async fn follow(client: &BrokerClient, response: &Response<Bytes>, relation: &RelationType) -> Result<Option<Answer>, UncheckedPageError> {
    let Some(link) = link(response.headers())? else {
        return Ok(None);
    };
    if link.find_by_rel(relation).next().is_none() {
        return Ok(None);
    }
    let followed = request(client, &link, relation)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    Ok(Some(client.send_unchecked(followed).await))
}

#[cfg(test)]
mod tests {
    use crate::unchecked_page::entities;
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;

    fn answer(body: &str) -> Result<Response<Bytes>> {
        Ok(Response::builder().status(200).body(Bytes::from(body.to_owned()))?)
    }

    #[gtest]
    fn the_entities_of_an_array_body_are_read() -> Result<()> {
        let response = answer(r#"[{"id":"urn:ngsi-ld:Building:1","type":"Building"}]"#)?;
        verify_that!(entities(&response)?.len(), eq(1))
    }

    #[gtest]
    fn a_body_that_is_not_an_array_is_refused() -> Result<()> {
        let response = answer(r#"{"id":"urn:ngsi-ld:Building:1","type":"Building"}"#)?;
        verify_that!(entities(&response).is_err(), eq(true))
    }
}
