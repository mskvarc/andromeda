//! Retrieve Temporal Evolution in aggregated representation (clause 4.5.19) answered to a test that reads the answer
//! itself: an aggregate the broker answers as `null` (a method that yields nothing over the period) cannot be held by
//! the typed aggregated reader, and a page reached through a `Link` is a body, not an answer of the typed client. The
//! request is the typed operation, sent unchecked; the answer comes back as received.

use crate::broker::BrokerClient;
use bytes::Bytes;
use http::Response;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::operation::{Aggregated, RetrieveTemporalEvolution, TemporalSelection},
    model::{
        DocumentError,
        EntityId,
        evolution::{AggregatedTemporalEntity, TemporalAttributeName},
    },
    query::temporal_query::AggregationMethods,
};
use std::str::{self, Utf8Error};

/// Why an aggregated answer could not be fetched or read.
#[derive(Debug, thiserror::Error)]
pub enum AggregatedAnswerError {
    /// The request could not be built or sent, or the broker refused it.
    #[error("the request failed")]
    Client(#[source] Box<ClientError<ReqwestError>>),
    /// The body is not UTF-8.
    #[error("the body is not UTF-8")]
    Utf8(#[from] Utf8Error),
    /// The body is not an aggregated temporal entity.
    #[error("the body is not an aggregated temporal entity")]
    Document(#[from] DocumentError),
}

/// The selection of a retrieve with no time bound that aggregates by `methods`.
#[must_use]
pub const fn unbounded(methods: &AggregationMethods) -> TemporalSelection<'_> {
    TemporalSelection::Unbounded {
        time_property: None,
        last_n: None,
        aggregation_methods: Some(methods),
        aggregation_period: None,
    }
}

/// Retrieve Temporal Evolution of the entity `id` in aggregated representation with `selection`, keeping the
/// attributes `attrs` (all when empty), answered as received.
///
/// # Errors
///
/// Returns [`AggregatedAnswerError::Client`] when the request cannot be built or sent, or the broker answers with an
/// error.
pub async fn retrieve_unread(
    client: &BrokerClient,
    id: &EntityId,
    selection: TemporalSelection<'_>,
    attrs: &[TemporalAttributeName],
) -> Result<Response<Bytes>, AggregatedAnswerError> {
    let operation = RetrieveTemporalEvolution::builder()
        .id(id)
        .selection(selection)
        .attrs(attrs)
        .format(Aggregated)
        .build();
    let request = client.unchecked(&operation).map_err(|error| AggregatedAnswerError::Client(Box::new(error)))?;
    client
        .send_unchecked(request)
        .await
        .map_err(|error| AggregatedAnswerError::Client(Box::new(error)))
}

/// The aggregated temporal entity `response` carries.
///
/// # Errors
///
/// Returns [`AggregatedAnswerError`] when the body is not UTF-8 or not an aggregated temporal entity.
pub fn read(response: &Response<Bytes>) -> Result<AggregatedTemporalEntity, AggregatedAnswerError> {
    Ok(AggregatedTemporalEntity::parse(str::from_utf8(response.body())?)?)
}

#[cfg(test)]
mod tests {
    use crate::aggregated_answer::{AggregatedAnswerError, read, unbounded};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;
    use ngsi_ld::{model::evolution::AggregationMethod, query::temporal_query::AggregationMethods};

    #[gtest]
    fn a_body_is_read_as_an_aggregated_temporal_entity() -> Result<()> {
        let answer = Response::new(Bytes::from_static(
            br#"{"id":"urn:ngsi-ld:Agg:1","type":"Agg","speed":{"type":"Property","sum":[[3,"2024-01-01T00:00:00Z","2024-01-01T01:00:00Z"]]}}"#,
        ));
        verify_that!(read(&answer)?.attributes.len(), eq(1))
    }

    #[gtest]
    fn a_body_that_is_not_an_aggregated_entity_is_refused() {
        let answer = Response::new(Bytes::from_static(br#"{"type":"Agg"}"#));
        expect_that!(
            read(&answer),
            err(predicate(|error: &AggregatedAnswerError| matches!(error, AggregatedAnswerError::Document(_))))
        );
    }

    #[gtest]
    fn an_unbounded_selection_names_the_methods_alone() -> Result<()> {
        let methods = AggregationMethods::new([AggregationMethod::Avg]).or_fail()?;
        let selection = unbounded(&methods);
        verify_that!(selection.last_n(), none())
    }
}
