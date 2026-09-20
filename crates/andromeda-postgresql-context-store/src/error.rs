//! Error types for the context store crate.

use crate::{entity::types_present::EntityTypesMissing, jsonld_context::mapper::ContextMapperError, schema_contract::error::SchemaContractError};
use andromeda_postgresql_collation::provisioned_collations::CollationProvisioningError;
use andromeda_postgresql_encoding::{copy::CopyFieldError, ewkb::EwkbError};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
use andromeda_postgresql_pool::is_pool_saturated;
use andromeda_postgresql_query::error::RenderError;
use sqlx::migrate::MigrateError;

/// Errors from the PostgreSQL context store.
///
/// Variants that carry the PG error code directly (`UniqueViolation`,
/// `InvalidRegex`) separate the two codes the broker's policy layer
/// currently inspects from the generic `Database` bucket, so the
/// `From<ContextStoreError>` impl in the binary can map them to the
/// correct HTTP status without re-inspecting the driver error.
#[derive(Debug, thiserror::Error)]
pub enum ContextStoreError {
    /// A database operation failed.
    #[error("context store: {0}")]
    Database(sqlx::Error),
    /// EWKB geometry encoding failed.
    #[error("context store encoding: {0}")]
    Encoding(#[from] EwkbError),
    /// An ICU collation a clause-4.23 ordering named could not be created.
    #[error("context store collation: {0}")]
    Collation(#[from] CollationProvisioningError),
    /// A value could not be staged as a binary COPY field: it is longer than
    /// the format's 32-bit field prefix can announce, or it does not
    /// serialise.
    #[error("context store COPY field: {0}")]
    CopyField(#[from] CopyFieldError),
    /// A `23505` unique-violation raised by the database.
    #[error("context store unique violation: {0}")]
    UniqueViolation(sqlx::Error),
    /// A `2201B` invalid-regular-expression raised by the database.
    #[error("context store regex: {0}")]
    InvalidRegex(sqlx::Error),
    /// A connection could not be acquired from the pool within its acquire
    /// deadline. The pool is at capacity, not broken: the request is shed as
    /// backpressure and can be retried unchanged, so policy answers it 503 with
    /// `Retry-After` rather than 500.
    #[error("context store connection pool saturated")]
    Saturated,
    /// A SQL statement could not be rendered from the compiled query.
    #[error("context store render: {0}")]
    Render(#[from] RenderError),
    /// A name dictionary resolution failed, which includes a resolution
    /// refused for taking the tenant's dictionary past its bound.
    #[error("context store name lookup: {0}")]
    NameLookup(#[from] NameLookupError),
    /// An entity write carried no type, which clause 4.5.1 forbids and the
    /// `entities` table refuses.
    #[error("context store entity types: {0}")]
    EntityTypes(#[from] EntityTypesMissing),
    /// A JSON-LD context row contained an invalid domain value.
    #[error("context store JSON-LD context row: {0}")]
    ContextMapper(#[from] ContextMapperError),
    /// A database migration failed.
    #[error("context store migration: {0}")]
    Migration(#[from] MigrateError),
    /// A tenant database is not the schema this binary was built against.
    #[error("context store schema contract: {0}")]
    SchemaContract(#[from] SchemaContractError),
    /// An internal invariant was violated (malformed stored data, serialization
    /// failure, etc.).
    #[error("context store internal: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for ContextStoreError {
    fn from(error: sqlx::Error) -> ContextStoreError {
        if is_pool_saturated(&error) {
            return ContextStoreError::Saturated;
        }
        if let sqlx::Error::Database(ref db_error) = error {
            match db_error.code().as_deref() {
                Some("23505") => return ContextStoreError::UniqueViolation(error),
                Some("2201B") => return ContextStoreError::InvalidRegex(error),
                Some(_) | None => {}
            }
        }
        ContextStoreError::Database(error)
    }
}

impl ContextStoreError {
    /// Whether this failure is a saturated connection pool anywhere in a cold
    /// build's error tree, rather than a genuine fault.
    ///
    /// A pool-acquire timeout can surface as [`ContextStoreError::Saturated`], as
    /// a raw [`ContextStoreError::Database`], or wrapped inside the schema-contract
    /// read a cold load runs before serving. All are backpressure the caller sheds
    /// as a 503 with `Retry-After`, so a cold build that times out acquiring a
    /// connection at any of these points is classified the same way.
    #[must_use]
    pub const fn is_pool_saturation(&self) -> bool {
        match self {
            ContextStoreError::Saturated => true,
            ContextStoreError::Database(error) => is_pool_saturated(error),
            ContextStoreError::SchemaContract(error) => error.is_pool_saturation(),
            ContextStoreError::UniqueViolation(_)
            | ContextStoreError::InvalidRegex(_)
            | ContextStoreError::Encoding(_)
            | ContextStoreError::Collation(_)
            | ContextStoreError::CopyField(_)
            | ContextStoreError::Render(_)
            | ContextStoreError::NameLookup(_)
            | ContextStoreError::EntityTypes(_)
            | ContextStoreError::ContextMapper(_)
            | ContextStoreError::Migration(_)
            | ContextStoreError::Internal(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::ContextStoreError, schema_contract::error::SchemaContractError};

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(ContextStoreError::from(sqlx::Error::PoolTimedOut), ContextStoreError::Saturated));
    }

    #[test]
    fn other_database_failures_are_not_saturation() {
        assert!(matches!(ContextStoreError::from(sqlx::Error::RowNotFound), ContextStoreError::Database(_)));
        assert!(matches!(ContextStoreError::from(sqlx::Error::PoolClosed), ContextStoreError::Database(_)));
    }

    #[test]
    fn a_cold_build_pool_timeout_is_saturation() {
        // A pool-acquire timeout inside the cold-load schema-contract read arrives
        // wrapped, not as the flat `Saturated` variant; it must still classify as
        // saturation so the cold build is shed as a 503 rather than a 500.
        let wrapped = ContextStoreError::SchemaContract(SchemaContractError::Database(sqlx::Error::PoolTimedOut));
        assert!(wrapped.is_pool_saturation());
        assert!(ContextStoreError::Saturated.is_pool_saturation());
        // A genuine schema drift and an unrelated database fault are not saturation.
        let drift = ContextStoreError::SchemaContract(SchemaContractError::NoMigrations);
        assert!(!drift.is_pool_saturation());
        assert!(!ContextStoreError::Database(sqlx::Error::RowNotFound).is_pool_saturation());
    }
}
