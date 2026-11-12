//! Retrieve Temporal Evolution in normalized representation answered to a test that reads the answer itself: an
//! evolution whose instances change kind (a Property, then a Relationship) is refused by the typed reader, which holds
//! the instances of one attribute as one kind. The request is the typed operation, sent unchecked; the answer comes
//! back as received.

use crate::broker::BrokerClient;
use bytes::Bytes;
use http::Response;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::operation::{NormalizedTemporal, RetrieveTemporalEvolution, TemporalSelection},
    model::EntityId,
};

/// Why an evolution could not be fetched.
#[derive(Debug, thiserror::Error)]
#[error("the request failed")]
pub struct EvolutionAnswerError(#[source] Box<ClientError<ReqwestError>>);

/// The selection of a retrieve with no time bound, no `lastN` and no aggregation: the broker's default range.
#[must_use]
pub const fn whole_history() -> TemporalSelection<'static> {
    TemporalSelection::Unbounded {
        time_property: None,
        last_n: None,
        aggregation_methods: None,
        aggregation_period: None,
    }
}

/// Retrieve Temporal Evolution of the entity `id` in normalized representation with `selection`, answered as received.
///
/// # Errors
///
/// Returns [`EvolutionAnswerError`] when the request cannot be built or sent, or the broker answers with an error.
pub async fn retrieve_unread(client: &BrokerClient, id: &EntityId, selection: TemporalSelection<'_>) -> Result<Response<Bytes>, EvolutionAnswerError> {
    let operation = RetrieveTemporalEvolution::builder()
        .id(id)
        .selection(selection)
        .format(NormalizedTemporal)
        .build();
    let request = client.unchecked(&operation).map_err(|error| EvolutionAnswerError(Box::new(error)))?;
    client.send_unchecked(request).await.map_err(|error| EvolutionAnswerError(Box::new(error)))
}

#[cfg(test)]
mod tests {
    use crate::evolution_answer::whole_history;
    use googletest::prelude::*;

    #[gtest]
    fn the_whole_history_asks_for_no_bound() {
        expect_that!(whole_history().last_n(), none());
        expect_that!(whole_history().time_property(), none());
    }
}
