//! Conditions the temporal store's own statements and callers guarantee, and
//! what breaking one looks like.
//!
//! None of these is reachable from client input: each names an answer the
//! catalogue statements, the name dictionaries or the `TimescaleDB` catalogue
//! cannot give when the store is correct, so reaching one means the adapter or
//! the tenant's schema is wrong rather than the request.

use crate::schema_ddl::timescale_policy::HypertableName;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use std::num::TryFromIntError;
use thiserror::Error;

/// A contract inside the temporal store that its own statements broke.
#[derive(Debug, Error)]
pub enum TemporalStoreInvariant {
    /// An attribute name the dictionary was asked to resolve is missing from a
    /// resolution that returned successfully.
    #[error("attribute name {0} is missing from the dictionary after resolution")]
    AttributeNameUnresolved(AttributeName),
    /// An entity type the dictionary was asked to resolve is missing from a
    /// resolution that returned successfully.
    #[error("entity type {0} is missing from the dictionary after resolution")]
    EntityTypeUnresolved(EntityTypeName),
    /// A count was requested but the count statement was not built.
    #[error("the temporal count statement was requested but not built")]
    CountStatementMissing,
    /// The count statement, an aggregate without grouping, answered no row.
    #[error("the temporal count statement answered no row")]
    CountRowMissing,
    /// A count does not fit the width it is reported at.
    #[error("a temporal store count is out of range: {0}")]
    CountOutOfRange(#[source] TryFromIntError),
    /// The `TimescaleDB` catalogue does not know a temporal table as a
    /// hypertable.
    #[error("{0} is not a hypertable the columnstore can be enabled on")]
    NotAHypertable(HypertableName),
    /// A hypertable declares no time dimension whose chunk interval could be
    /// reconciled.
    #[error("{0} declares no time dimension to reconcile")]
    NoTimeDimension(HypertableName),
}
