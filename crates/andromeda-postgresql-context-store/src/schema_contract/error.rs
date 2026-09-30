//! What a schema-contract check refuses a tenant for.

use crate::schema_contract::{
    migrations_digest::{MigrationsDigest, MigrationsDigestError},
    schema_version::{SchemaVersion, SchemaVersionError},
    server_encoding::ServerEncoding,
    server_requirements::{ExtensionPresence, ExtensionVersion, ExtensionVersionError, PostgresVersion},
};
use andromeda_postgresql_pool::is_pool_saturated;

/// Why a tenant database is not the one this binary was built against.
///
/// Every mismatch class is its own variant and every variant names what was
/// expected next to what was found, because the operator's next action differs
/// per class: a digest mismatch means the fleet is running two binaries, a
/// version mismatch means a tenant is behind, and a floor shortfall means the
/// server itself must be upgraded.
#[derive(Debug, thiserror::Error)]
pub enum SchemaContractError {
    /// The contract could not be read or written.
    #[error("tenant schema contract: {0}")]
    Database(#[from] sqlx::Error),
    /// The binary embeds no tenant migration, so there is no contract to stamp.
    #[error("tenant schema contract: the embedded tenant migrator carries no migration")]
    NoMigrations,
    /// A migration version is outside the domain the stamp column declares.
    #[error("tenant schema contract: {0}")]
    SchemaVersionValue(#[from] SchemaVersionError),
    /// The stored digest is not a digest.
    #[error("tenant schema contract: {0}")]
    MigrationsDigestValue(#[from] MigrationsDigestError),
    /// A reported extension version could not be read.
    #[error("tenant schema contract: {0}")]
    ExtensionVersionValue(#[from] ExtensionVersionError),
    /// The tenant holds no `schema_metadata` row: it was migrated but never
    /// stamped, which is drift rather than a state to repair silently.
    #[error(
        "tenant schema contract: the database carries no schema_metadata row; expected schema version {expected_version} and migrations digest {expected_digest}"
    )]
    Unstamped {
        /// The version this binary would have stamped.
        expected_version: SchemaVersion,
        /// The digest this binary would have stamped.
        expected_digest: MigrationsDigest,
    },
    /// The tenant is stamped with a different schema version.
    #[error("tenant schema contract: schema version mismatch, expected {expected} but found {found}")]
    SchemaVersionMismatch {
        /// The version this binary requires.
        expected: SchemaVersion,
        /// The version the tenant is stamped with.
        found: SchemaVersion,
    },
    /// The tenant is stamped with a different migration set.
    #[error("tenant schema contract: migrations digest mismatch, expected {expected} but found {found}")]
    MigrationsDigestMismatch {
        /// The digest this binary requires.
        expected: MigrationsDigest,
        /// The digest the tenant is stamped with.
        found: MigrationsDigest,
    },
    /// The server encoding is not UTF-8, so the byte bounds the migrations
    /// state measure different octets than the broker does.
    #[error("tenant schema contract: the server encoding is {found}, but the schema requires UTF8")]
    ServerEncodingNotUtf8 {
        /// The encoding the server reports.
        found: ServerEncoding,
    },
    /// The server reports an encoding name PostgreSQL does not list as a
    /// server-side character set.
    #[error("tenant schema contract: the server encoding is not a known PostgreSQL server encoding: {0}")]
    ServerEncodingUnreadable(#[source] strum::ParseError),
    /// The server is below the version the schema depends on.
    #[error("tenant schema contract: PostgreSQL {found} is below the required minimum {required}")]
    PostgresBelowMinimum {
        /// The minimum the schema depends on.
        required: PostgresVersion,
        /// What the server reports.
        found: PostgresVersion,
    },
    /// `PostGIS` is absent or below the version the schema depends on.
    #[error("tenant schema contract: PostGIS {found} is below the required minimum {required}")]
    PostGisBelowMinimum {
        /// The minimum the schema depends on.
        required: ExtensionVersion,
        /// What the extension catalogue reports.
        found: ExtensionPresence,
    },
    /// `TimescaleDB` is absent or below the version the schema depends on.
    #[error("tenant schema contract: TimescaleDB {found} is below the required minimum {required}")]
    TimescaleDbBelowMinimum {
        /// The minimum the schema depends on.
        required: ExtensionVersion,
        /// What the extension catalogue reports.
        found: ExtensionPresence,
    },
}

impl SchemaContractError {
    /// Whether this refusal is really a saturated connection pool rather than a
    /// schema fault.
    ///
    /// The contract read acquires a connection, and under a tight pooler that
    /// acquire can time out. Such a timeout is backpressure, not drift, so the
    /// caller sheds it as a 503 rather than refusing the tenant its schema.
    #[must_use]
    pub fn is_pool_saturation(&self) -> bool {
        match self {
            SchemaContractError::Database(error) => is_pool_saturated(error),
            SchemaContractError::NoMigrations
            | SchemaContractError::SchemaVersionValue(_)
            | SchemaContractError::MigrationsDigestValue(_)
            | SchemaContractError::ExtensionVersionValue(_)
            | SchemaContractError::Unstamped { .. }
            | SchemaContractError::SchemaVersionMismatch { .. }
            | SchemaContractError::MigrationsDigestMismatch { .. }
            | SchemaContractError::ServerEncodingNotUtf8 { .. }
            | SchemaContractError::ServerEncodingUnreadable(_)
            | SchemaContractError::PostgresBelowMinimum { .. }
            | SchemaContractError::PostGisBelowMinimum { .. }
            | SchemaContractError::TimescaleDbBelowMinimum { .. } => false,
        }
    }
}
