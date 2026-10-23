//! Why one entity a source answered is not read into the model.

use andromeda_ngsi_ld_expansion::{error::ExpansionError, received_state::ReceivedRefusal};
use andromeda_ngsi_ld_model::node_refusal::RefusedNode;
use thiserror::Error;

/// Why one entity of an answer of current entities is not read into the model.
#[derive(Debug, Error)]
pub(crate) enum EntityReading {
    /// The entity's own `@context` does not resolve or process.
    #[error("its own @context cannot be processed: {0}")]
    OwnContext(#[source] ExpansionError),
    /// The entity, read under the context it was written in, is no entity the
    /// model can hold.
    #[error(transparent)]
    Undecodable(RefusedNode),
    /// The entity does not read into the model.
    #[error(transparent)]
    Refused(ReceivedRefusal),
}

/// Why one entity of an answer of Temporal Evolutions is not read into the model.
#[derive(Debug, Error)]
pub(crate) enum TemporalEntityReading {
    /// The entity's own `@context` does not resolve or process.
    #[error("its own @context cannot be processed: {0}")]
    OwnContext(#[source] ExpansionError),
    /// The entity does not read into the model.
    #[error(transparent)]
    Refused(ExpansionError),
}
