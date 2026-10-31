//! Delete Attribute addressed to `scope` (clause 5.6.5), which the typed operation cannot name: the attribute name
//! type refuses the reserved member, so the operation is built for a placeholder and sent to the path as written.

use crate::{
    broker::BrokerClient,
    wire::{WireError, api_uri},
};
use bytes::Bytes;
use http::Response;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::operation::DeleteAttribute,
    model::{AttributeName, EntityId, NameError},
};

/// Why the scope of an entity could not be deleted.
#[derive(Debug, thiserror::Error)]
pub enum ScopeDeletionError {
    /// The placeholder name is not an attribute name.
    #[error(transparent)]
    Name(#[from] NameError),
    /// The request target is not a URI.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The request could not be built or sent.
    #[error("the request failed")]
    Client(#[source] Box<ClientError<ReqwestError>>),
}

/// Sends Delete Attribute for `scope` of the entity `id` through `client`, and answers with the response as received.
///
/// # Errors
///
/// Returns [`ScopeDeletionError`] when the request cannot be built or sent.
pub async fn delete_scope(client: &BrokerClient, id: &EntityId) -> Result<Response<Bytes>, ScopeDeletionError> {
    let placeholder = "placeholder".parse::<AttributeName>()?;
    let operation = DeleteAttribute::builder().id(id).name(&placeholder).build();
    let request = client
        .unchecked(&operation)
        .map_err(|error| ScopeDeletionError::Client(Box::new(error)))?
        .uri(api_uri(client, &format!("entities/{id}/attrs/scope"))?);
    client
        .send_unchecked(request)
        .await
        .map_err(|error| ScopeDeletionError::Client(Box::new(error)))
}
