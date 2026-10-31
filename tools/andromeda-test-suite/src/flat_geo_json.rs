//! Retrieve Entity with a flat join answered as `GeoJSON` (clauses 4.5.23.3, 4.5.16.3 and 6.3.15): a
//! `FeatureCollection` of the entity and the entities its Relationships link to. The client's typed retrieval
//! answers `GeoJSON` for one Feature only, so the request is written as text and the answer read through the model.

use crate::{
    broker::BrokerClient,
    wire::{WireError, request_to},
};
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::UncheckedRequest,
    model::{AttributeName, DocumentError, Entity, EntityId, FeatureCollection, Lossless, Output},
};
use std::str::{self, Utf8Error};

/// Why the collection could not be retrieved.
#[derive(Debug, thiserror::Error)]
pub enum FlatGeoJsonError {
    /// The request target is not a URI.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The client failed to send the request or the broker answered with an error.
    #[error("the request failed")]
    Client(#[from] ClientError<ReqwestError>),
    /// The broker answered with a status other than 200.
    #[error("the broker answered {0}")]
    Status(StatusCode),
    /// The body is not UTF-8 text.
    #[error(transparent)]
    Text(#[from] Utf8Error),
    /// The body is not a `FeatureCollection` of entities.
    #[error("the body is not a FeatureCollection")]
    Document(#[from] DocumentError),
}

/// The `FeatureCollection` Retrieve Entity answers for `id` with `join=flat` and `joinLevel=join_level`, its
/// geometries taken from `geometry_property` or from `location` when none is named.
///
/// # Errors
///
/// Returns [`FlatGeoJsonError`] when the request fails, the answer is not 200 or its body is not a collection.
pub async fn retrieve_flat_collection(
    client: &BrokerClient,
    id: &EntityId,
    join_level: u32,
    geometry_property: Option<&AttributeName>,
) -> Result<FeatureCollection<Entity<Output>>, FlatGeoJsonError> {
    let response = client.send_unchecked(flat_request(client, id, join_level, geometry_property)?).await?;
    if response.status() != StatusCode::OK {
        return Err(FlatGeoJsonError::Status(response.status()));
    }
    collection_of(response.body())
}

/// The Retrieve Entity request of [`retrieve_flat_collection`], accepting `application/geo+json`.
///
/// # Errors
///
/// Returns [`WireError`] when the request target is not a URI.
fn flat_request(client: &BrokerClient, id: &EntityId, join_level: u32, geometry_property: Option<&AttributeName>) -> Result<UncheckedRequest, WireError> {
    let geometry = geometry_property.map_or_else(String::new, |name| format!("&geometryProperty={name}"));
    Ok(
        request_to(client, Method::GET, &format!("entities/{id}?join=flat&joinLevel={join_level}{geometry}"))?
            .append_header(ACCEPT, HeaderValue::from_static("application/geo+json")),
    )
}

/// The collection a response body holds.
fn collection_of(body: &[u8]) -> Result<FeatureCollection<Entity<Output>>, FlatGeoJsonError> {
    FeatureCollection::<Entity<Output>>::parse(str::from_utf8(body)?, Lossless::Normalized).map_err(FlatGeoJsonError::Document)
}

#[cfg(test)]
mod tests {
    use crate::flat_geo_json::collection_of;
    use googletest::prelude::*;

    #[gtest]
    fn a_collection_of_one_feature_is_read() -> Result<()> {
        let collection = collection_of(
            br#"{"type":"FeatureCollection","features":[{"id":"urn:ngsi-ld:Building:1","type":"Feature",
            "geometry":{"type":"Point","coordinates":[0.5,0.25]},"properties":{"type":"Building"}}]}"#,
        )?;
        verify_that!(collection.features, len(eq(1)))
    }

    #[gtest]
    fn a_body_that_is_not_a_collection_is_refused() -> Result<()> {
        verify_that!(collection_of(b"[]"), err(anything()))
    }
}
