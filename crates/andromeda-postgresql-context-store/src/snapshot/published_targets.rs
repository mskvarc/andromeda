//! Listing the published branch of every Snapshot a tenant holds.
//!
//! A tenant runtime matches entity changes against the Subscriptions cached
//! per context target, and Subscriptions created under `NGSILD-Snapshot` live
//! in the Snapshot's own branch. A runtime built after the tenant was evicted
//! therefore loads every published branch beside the live one, which needs the
//! targets those branches are addressed by.

use crate::{snapshot::identities::read_snapshot_identities, store_fault::persistence_error};
use andromeda_context_space::snapshot_target::SnapshotTarget;
use andromeda_snapshot::error::SnapshotError;
use sqlx::PgPool;

/// The published branch of every Snapshot of the tenant `pool` serves.
pub(crate) async fn read_published_targets(pool: &PgPool) -> Result<Vec<SnapshotTarget>, SnapshotError> {
    Ok(read_snapshot_identities(pool)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?
        .into_iter()
        .map(|identity| SnapshotTarget::published(identity.key, identity.id))
        .collect())
}
