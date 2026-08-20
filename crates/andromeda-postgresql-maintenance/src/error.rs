//! Typed failures produced by platform coordination and tenant inspection.

use andromeda_ngsi_ld_lexicon::tenant::InvalidTenantName;
use andromeda_postgresql_context_store::schema_contract::error::SchemaContractError;
use sea_query::error::Error as SeaQueryError;
use thiserror::Error;

/// A controller pass could not complete its required platform operation.
#[derive(Debug, Error)]
pub enum MaintenanceError {
    /// Platform or tenant PostgreSQL interaction failed.
    #[error("PostgreSQL maintenance statement failed: {0}")]
    Database(#[from] sqlx::Error),
    /// A tenant schema does not match the running binary.
    #[error("PostgreSQL maintenance skipped tenant schema: {0}")]
    SchemaContract(#[from] SchemaContractError),
    /// A catalog counter contradicted PostgreSQL's non-negative invariant.
    #[error("PostgreSQL catalog returned a negative value for {field}: {value}")]
    NegativeCatalogValue {
        /// Catalog field whose value was invalid.
        field: &'static str,
        /// Negative value returned by PostgreSQL.
        value: i64,
    },
    /// A non-negative catalog value cannot fit the platform schema's signed type.
    #[error("PostgreSQL catalog value for {field} exceeds the platform state range: {value}")]
    CatalogValueOverflow {
        /// Catalog field whose value could not be represented.
        field: &'static str,
        /// Unsigned value returned by PostgreSQL.
        value: u64,
    },
    /// SeaQuery rejected an internally assembled statement.
    #[error("PostgreSQL maintenance statement assembly failed: {0}")]
    Statement(#[from] SeaQueryError),
    /// The platform registry contains an invalid tenant name.
    #[error("platform tenant registry contains an invalid tenant name: {0}")]
    TenantName(#[from] InvalidTenantName),
    /// Durable state named a target this binary does not own.
    #[error("platform maintenance state contains unknown target {0:?}")]
    UnknownTarget(String),
}
