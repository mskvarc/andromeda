//! Retrieve Temporal Evolution of an Entity that makes an entity map and answers 201 (clause 5.7.3.4): the typed
//! retrieval reads 200, 203 and 206 only, so the typed request is sent unchecked and the answer is read here.

use crate::{broker::BrokerClient, received_status::UnexpectedStatus};
use bytes::Bytes;
use http::{Response, StatusCode};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{
        ResponseHead,
        headers::NgsiLdEntityMap,
        operation::{EntityMapRequest, NormalizedTemporal, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{DocumentError, EntityId, Lossless, Output, evolution::TemporalEntity},
    query::temporal_query::TemporalQuery,
};
use std::str::{self, Utf8Error};

/// A temporal evolution answered together with the entity map the broker made for it.
#[derive(Debug, Clone)]
pub struct MadeMap {
    /// The evolution of the answer body.
    pub evolution: TemporalEntity<Output>,
    /// The `NGSILD-EntityMap` header of the answer.
    pub map: NgsiLdEntityMap,
}

/// Why a retrieval that makes an entity map could not be read.
#[derive(Debug, thiserror::Error)]
pub enum MadeMapError {
    /// The request could not be built or sent, or the broker refused it.
    #[error("the request failed")]
    Client(#[source] Box<ClientError<ReqwestError>>),
    /// The broker answered a success other than 201.
    #[error("the answer is not the one that makes a map")]
    Status(#[from] UnexpectedStatus),
    /// The body is not UTF-8.
    #[error("the body is not UTF-8")]
    Utf8(#[from] Utf8Error),
    /// The body is not a normalized temporal entity.
    #[error("the body is not a temporal entity")]
    Document(#[from] DocumentError),
    /// The answer carries no readable `NGSILD-EntityMap` header.
    #[error("the answer carries no readable NGSILD-EntityMap header")]
    MissingMap,
}

/// Retrieve Temporal Evolution of `id` over `query`, asking `request` for an entity map, and require the 201 that
/// answers a map made.
///
/// # Errors
///
/// Returns [`MadeMapError`] when the request fails, the answer is not 201, its body is not a temporal entity or it
/// names no entity map.
pub async fn retrieve_making_map(client: &BrokerClient, id: &EntityId, query: &TemporalQuery, request: &EntityMapRequest) -> Result<MadeMap, MadeMapError> {
    let operation = RetrieveTemporalEvolution::builder()
        .id(id)
        .selection(TemporalSelection::Query(query))
        .entity_map(request)
        .format(NormalizedTemporal)
        .build();
    let unchecked = client.unchecked(&operation).map_err(|error| MadeMapError::Client(Box::new(error)))?;
    let response = client.send_unchecked(unchecked).await.map_err(|error| MadeMapError::Client(Box::new(error)))?;
    read(&response)
}

/// The evolution and the entity map a 201 answer carries.
///
/// # Errors
///
/// Returns [`MadeMapError`] when the answer is not 201, its body is not a temporal entity or it names no entity map.
pub fn read(response: &Response<Bytes>) -> Result<MadeMap, MadeMapError> {
    if response.status() != StatusCode::CREATED {
        return Err(UnexpectedStatus {
            expected: StatusCode::CREATED,
            actual: response.status(),
        }
        .into());
    }
    let evolution = TemporalEntity::<Output>::parse(str::from_utf8(response.body())?, Lossless::Normalized)?;
    let map = ResponseHead::new(response.headers().clone())
        .typed::<NgsiLdEntityMap>()
        .into_valid()
        .ok_or(MadeMapError::MissingMap)?;
    Ok(MadeMap { evolution, map })
}

#[cfg(test)]
mod tests {
    use crate::temporal_map_retrieval::{MadeMapError, read};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{Response, StatusCode};

    const BODY: &str = r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[{"type":"Property","value":50,"observedAt":"2020-08-01T12:00:00Z"}]}"#;

    fn answer(status: StatusCode, map: Option<&str>) -> Result<Response<Bytes>> {
        let mut builder = Response::builder().status(status);
        if let Some(map) = map {
            builder = builder.header("NGSILD-EntityMap", map);
        }
        Ok(builder.body(Bytes::from_static(BODY.as_bytes()))?)
    }

    #[gtest]
    fn a_created_answer_with_a_map_header_is_read() -> Result<()> {
        let made = read(&answer(StatusCode::CREATED, Some("urn:ngsi-ld:EntityMap:1"))?)?;
        verify_that!(made.map.id(), some(anything()))
    }

    #[gtest]
    fn an_answer_that_is_not_created_is_refused() -> Result<()> {
        verify_that!(
            read(&answer(StatusCode::OK, Some("urn:ngsi-ld:EntityMap:1"))?),
            err(predicate(|error: &MadeMapError| matches!(error, MadeMapError::Status(_))))
        )
    }

    #[gtest]
    fn a_created_answer_without_a_map_header_is_refused() -> Result<()> {
        verify_that!(
            read(&answer(StatusCode::CREATED, None)?),
            err(predicate(|error: &MadeMapError| matches!(error, MadeMapError::MissingMap)))
        )
    }
}
