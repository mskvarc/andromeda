//! Temporal evolution calls whose body or query is written down to the byte, for a test whose subject is a
//! temporal representation the typed temporal model cannot write or read (a `JsonProperty` instance, `jsons` pairs, a
//! single instance object, an aggregated answer with a null period).

use crate::{
    broker::BrokerClient,
    raw_call::{RawCallError, call, target},
};
use bytes::Bytes;
use http::{Method, Response};
use std::fmt::Display;

/// Create or Update Temporal Evolution of an Entity with `body` byte for byte.
///
/// # Errors
///
/// Returns [`RawCallError`] when the request cannot be written, sent or answered.
pub async fn create(broker: &BrokerClient, body: &str) -> Result<Response<Bytes>, RawCallError> {
    call(broker, Method::POST, "temporal/entities", &[], Some(body)).await
}

/// Add Attributes to Temporal Evolution of the Entity `id` with `body` byte for byte.
///
/// # Errors
///
/// Returns [`RawCallError`] when the request cannot be written, sent or answered.
pub async fn append(broker: &BrokerClient, id: impl Display, body: &str) -> Result<Response<Bytes>, RawCallError> {
    call(broker, Method::POST, &format!("temporal/entities/{id}/attrs"), &[], Some(body)).await
}

/// Retrieve Temporal Evolution of the Entity `id` as `application/json` with the query `parameters`.
///
/// # Errors
///
/// Returns [`RawCallError`] when the request cannot be written, sent or answered.
pub async fn retrieve(broker: &BrokerClient, id: impl Display, parameters: &[(&str, &str)]) -> Result<Response<Bytes>, RawCallError> {
    call(
        broker,
        Method::GET,
        &target(&format!("temporal/entities/{id}"), parameters),
        &[("Accept", "application/json")],
        None,
    )
    .await
}

/// Query Temporal Evolution of Entities as `application/json` with the query `parameters`.
///
/// # Errors
///
/// Returns [`RawCallError`] when the request cannot be written, sent or answered.
pub async fn query(broker: &BrokerClient, parameters: &[(&str, &str)]) -> Result<Response<Bytes>, RawCallError> {
    call(
        broker,
        Method::GET,
        &target("temporal/entities", parameters),
        &[("Accept", "application/json")],
        None,
    )
    .await
}

/// Query Temporal Evolution of Entities by POST with the query document `body` byte for byte.
///
/// # Errors
///
/// Returns [`RawCallError`] when the request cannot be written, sent or answered.
pub async fn query_by_post(broker: &BrokerClient, parameters: &[(&str, &str)], body: &str) -> Result<Response<Bytes>, RawCallError> {
    call(
        broker,
        Method::POST,
        &target("temporal/entityOperations/query", parameters),
        &[("Accept", "application/json"), ("Content-Type", "application/json")],
        Some(body),
    )
    .await
}
