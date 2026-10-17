//! Typed failures produced by platform coordination and tenant inspection.

use crate::{candidate::MaintenanceTarget, catalog_counter::CatalogCounter};
use andromeda_context_space::snapshot_key::{SnapshotKey, SnapshotKeyError};
use andromeda_ngsi_ld_lexicon::tenant::InvalidTenantName;
use andromeda_postgresql_codes::{maintenance_target::MaintenanceTargetCode, registry::UnknownCompactCode};
use andromeda_postgresql_context_store::{error::ContextStoreError, schema_contract::error::SchemaContractError};
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
    #[error("PostgreSQL catalog returned a negative value for {counter}: {value}")]
    NegativeCatalogValue {
        /// Counter whose value was invalid.
        counter: CatalogCounter,
        /// Negative value returned by PostgreSQL.
        value: i64,
    },
    /// A non-negative catalog value cannot fit the platform schema's signed type.
    #[error("PostgreSQL catalog value for {counter} exceeds the platform state range: {value}")]
    CatalogValueOverflow {
        /// Counter whose value could not be represented.
        counter: CatalogCounter,
        /// Unsigned value returned by PostgreSQL.
        value: u64,
    },
    /// `SeaQuery` rejected an internally assembled statement.
    #[error("PostgreSQL maintenance statement assembly failed: {0}")]
    Statement(#[from] SeaQueryError),
    /// The platform registry contains an invalid tenant name.
    #[error("platform tenant registry contains an invalid tenant name: {0}")]
    TenantName(#[from] InvalidTenantName),
    /// The catalog named a relation this binary does not own as a target.
    #[error("PostgreSQL catalog reported a relation outside the maintenance allowlist: {0}")]
    UnknownTarget(#[source] strum::ParseError),
    /// Durable state holds a code no release assigned.
    #[error("platform maintenance state is unreadable: {0}")]
    UnreadableCode(#[from] UnknownCompactCode),
    /// A replication slot reported a `wal_status` this binary does not know.
    #[error("PostgreSQL reported an unknown replication slot WAL status: {0}")]
    UnknownWalStatus(#[source] strum::ParseError),
    /// The installed and shipped versions of a tenant's extensions could not
    /// be read.
    #[error("PostgreSQL extension versions could not be read: {0}")]
    ExtensionVersions(#[source] ContextStoreError),
    /// The vacuum horizon report named a holder kind this binary never asks for.
    #[error("PostgreSQL vacuum horizon report named an unknown holder kind: {0}")]
    UnknownHorizonHolder(#[source] strum::ParseError),
    /// Durable index-probe state names a target that is not an index.
    #[error("platform maintenance state records an index probe for non-index target '{}'", .0.label())]
    ProbeTargetNotAnIndex(MaintenanceTargetCode),
    /// Durable state records a density baseline that is not positive, or on a
    /// target no density measures.
    #[error("platform maintenance state records an invalid density baseline {value} for '{}'", .target.label())]
    InvalidDensityBaseline {
        /// The target the baseline is recorded on.
        target: MaintenanceTargetCode,
        /// The stored baseline.
        value: i64,
    },
    /// A catalog statement selecting branch schemas returned a schema that is
    /// neither the live nor a published Snapshot branch.
    #[error("PostgreSQL catalog returned a schema that is no maintenance branch")]
    UnknownBranchSchema,
    /// A stored Snapshot key, in the platform target rows or a tenant's
    /// Snapshot control records, is not positive.
    #[error("stored Snapshot key is invalid: {0}")]
    InvalidSnapshotKey(#[from] SnapshotKeyError),
    /// A tenant's Snapshot control record holds a status no release assigned.
    #[error("tenant Snapshot control record holds an unknown status: {0}")]
    UnreadableSnapshotStatus(#[source] UnknownCompactCode),
    /// A Snapshot branch lock this session took was no longer held when it
    /// released it.
    #[error("the PostgreSQL maintenance lock of Snapshot {} branch was not held", .snapshot.as_i64())]
    BranchLockNotHeld {
        /// The Snapshot whose branch lock was released.
        snapshot: SnapshotKey,
    },
    /// A target was addressed in a Snapshot branch whose schema never holds
    /// its heap.
    #[error("maintenance target '{}' exists only in the live branch, not in Snapshot {}", .target.relation_name(), .snapshot.as_i64())]
    LiveOnlyTarget {
        /// The live-only target.
        target: MaintenanceTarget,
        /// The Snapshot branch it was addressed in.
        snapshot: SnapshotKey,
    },
}
