//! Failures that belong to rendering rather than to the query language.

use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_postgresql_encoding::ewkb::EwkbError;

/// Errors raised while rendering a compiled query as PostgreSQL.
///
/// The language's own failures arrive through [`QueryError`]: a renderer is
/// where clause 4.9's operator eligibility and clause 4.23's ordering rules are
/// finally checked, so they surface here rather than at parse time. The
/// remaining variant is this backend's alone and is the reason this enum exists
/// — PostGIS's wire format is not knowable to a specification crate.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The query itself was invalid, detected while rendering it.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// The reference geometry could not be encoded for the store.
    #[error("geo: reference geometry could not be encoded")]
    ReferenceGeometryEncoding(#[from] EwkbError),
}
