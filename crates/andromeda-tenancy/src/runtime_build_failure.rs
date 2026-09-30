//! Why a registered tenant's runtime could not be built.

use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use thiserror::Error;

/// Why a tenant's runtime could not be built.
///
/// The composition root that builds the runtime lives in a crate this one cannot
/// name, so it classifies its own failure into this closed vocabulary and records
/// the failure's detail for operators where it classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RuntimeBuildFailure {
    /// A store the runtime depends on could not be reached, migrated or read.
    #[error(transparent)]
    Persistence(#[from] PersistenceFailure),
    /// The tenant's JSON-LD engine could not be built.
    #[error("the tenant's JSON-LD engine could not be built")]
    JsonLdEngine,
    /// A construction invariant of the runtime could not be established.
    #[error("a tenant runtime construction invariant could not be established")]
    Invariant,
}
