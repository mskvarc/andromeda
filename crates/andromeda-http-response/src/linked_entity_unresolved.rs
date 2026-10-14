//! The text of the clause 6.3.17 warning an unresolved Linked Entity is reported with.

use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use thiserror::Error;

/// A Linked Entity a join could not resolve.
#[derive(Debug, Error)]
#[error("linked entity {} could not be resolved: {}", .0.id, .0.detail)]
pub(crate) struct LinkedEntityUnresolved<'a>(pub(crate) &'a LinkedResolutionFailure);
