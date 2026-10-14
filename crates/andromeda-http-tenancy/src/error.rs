//! Focused failures produced by tenant management over HTTP.

use andromeda_http_extraction::classification::classify_tenancy_error;
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    tenant::InvalidTenantName,
};
use andromeda_tenancy::error::TenancyError;

/// A tenant path segment that is not a valid tenant name.
#[derive(Debug, thiserror::Error)]
#[error("invalid tenant name: {0}")]
pub(crate) struct InvalidTenantPathName(#[source] pub(crate) InvalidTenantName);

/// A pagination cursor boundary that is not a tenant resource identifier.
#[derive(Debug, thiserror::Error)]
#[error("the pagination cursor does not address a tenant: {id}")]
pub(crate) struct CursorNotATenant<'id> {
    /// The boundary identifier the cursor carries.
    pub(crate) id: &'id Identifier,
}

/// Failures local to the tenancy HTTP binding.
#[derive(Debug, thiserror::Error)]
pub enum TenancyHttpError {
    /// The tenant registry, runtime cache or usage probe rejected the operation.
    #[error(transparent)]
    Tenancy(#[from] TenancyError),
    /// The tenant resource identifier could not be constructed from its name,
    /// which a validated tenant name never triggers in practice.
    #[error("the tenant resource identifier could not be constructed")]
    TenantId(#[from] IdentifierParseError),
}

/// Classify every tenancy-binding failure exhaustively.
#[must_use]
pub fn classify(error: &TenancyHttpError) -> ClassifiedReport {
    match error {
        TenancyHttpError::Tenancy(source) => classify_tenancy_error(source),
        TenancyHttpError::TenantId(_) => ClassifiedReport::internal(error, InternalFailureSummary::IDENTIFIER_RESOLUTION),
    }
}
