//! Failures that belong to rendering rather than to the query language.

use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_postgresql_encoding::ewkb::EwkbError;
use andromeda_postgresql_seaquery::bind::BindConversionError;

/// Errors raised while rendering a compiled query as PostgreSQL.
///
/// The language's own failures arrive through [`QueryError`]: a renderer is
/// where clause 4.9's operator eligibility and clause 4.23's ordering rules are
/// finally checked, so they surface here rather than at parse time. The other
/// two variants are this backend's alone and are the reason this enum exists —
/// neither a bind-value mapping nor PostGIS's wire format is knowable to a
/// specification crate.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The query itself was invalid, detected while rendering it.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// A sea-query value had no typed bind counterpart, so binding it would
    /// have changed the comparison semantics.
    #[error(transparent)]
    BindConversion(#[from] BindConversionError),
    /// The reference geometry could not be encoded for the store.
    #[error("geo: reference geometry could not be encoded")]
    ReferenceGeometryEncoding(#[from] EwkbError),
}
