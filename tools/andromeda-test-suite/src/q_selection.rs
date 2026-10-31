//! The entities a `q` selects among an entity the test created, for a test whose subject is which spellings of the
//! query language (clause 4.9) pick the entity and which do not.

use crate::broker::BrokerClient;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::headers::Accept,
    model::{BuildError, EntityId, EntityTypeName, NgsiLdMediaType},
    query::{ParseError, Problem, q::Query, type_selection},
};

/// Why the selection could not be asked.
#[derive(Debug, thiserror::Error)]
pub enum QSelectionError {
    /// The text is not a query of the grammar.
    #[error("the text is not a query")]
    Query(#[source] ParseError),
    /// The broker refused the query or the call failed.
    #[error("the query was not answered")]
    Client(#[source] Box<ClientError<ReqwestError>>),
    /// The type selection is not valid.
    #[error("the type selection is not valid")]
    Selection(#[source] Box<BuildError<Problem>>),
}

/// The identifiers of the entities of type `entity_type` that the query written as `q` selects, in the order of the
/// answer.
///
/// # Errors
///
/// Returns [`QSelectionError`] when `q` is not a query, the selection is not valid or the broker does not answer with a
/// page.
pub async fn selected_ids(broker: &BrokerClient, entity_type: &EntityTypeName, q: &str) -> Result<Vec<EntityId>, QSelectionError> {
    let query = q.parse::<Query>().map_err(QSelectionError::Query)?;
    let selection = type_selection::entity_type(entity_type)
        .build()
        .map_err(|error| QSelectionError::Selection(Box::new(error)))?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(selection)
        .q(query)
        .send()
        .await
        .map_err(|error| QSelectionError::Client(Box::new(error)))?;
    Ok(page.items().iter().map(|entity| entity.id.clone()).collect())
}

/// The number of entities of the answer to Query Entities for the entity `id` and the query written as `q`.
///
/// # Errors
///
/// Returns [`QSelectionError`] when `q` is not a query or the broker does not answer with a page.
pub async fn selected_count(broker: &BrokerClient, id: &EntityId, q: &str) -> Result<usize, QSelectionError> {
    let query = q.parse::<Query>().map_err(QSelectionError::Query)?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![id.clone()])
        .q(query)
        .send()
        .await
        .map_err(|error| QSelectionError::Client(Box::new(error)))?;
    Ok(page.items().len())
}
