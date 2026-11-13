//! What a tenant database has applied: the migrations `public`'s ledger
//! records, and every Snapshot branch schema with the migrations its own
//! ledger records.
//!
//! The Snapshot branches are the ones a control record names, published or
//! staging, whose schema exists. A schema without a control record is never
//! read or written again, and the expiry sweep drops it, so it is not migrated.

use crate::{
    error::ContextStoreError,
    lockstep_migration::{plan::SnapshotProgress, snapshot_migration_error::SnapshotMigrationError},
    migration_set::{branch::branch_ledger, migration_version::MigrationVersion, tenant::TENANT_LEDGER},
    snapshot::identities::{SnapshotIdentity, read_snapshot_identities},
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_target::SnapshotTarget};
use andromeda_postgresql_branch::snapshot_schema_name;
use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_postgresql_seaquery::{
    introspection::{relation_exists, schema_exists},
    statement::Statement,
};
use sea_query::{Iden, Query, SimpleExpr};
use sqlx::{
    PgConnection,
    migrate::{AppliedMigration, Migrate},
};
use std::collections::BTreeSet;

/// One Snapshot branch schema and how far it has come.
#[derive(Debug)]
struct SurveyedBranch {
    /// The branch.
    branch: SnapshotTarget,
    /// The highest migration its ledger records, `None` when it records none.
    applied_through: Option<MigrationVersion>,
}

/// What the ledgers of a tenant database record.
#[derive(Debug)]
pub(crate) struct TenantSurvey {
    /// The migrations `public`'s ledger records.
    public_applied: BTreeSet<MigrationVersion>,
    /// Every Snapshot branch schema, in Snapshot key order, the published
    /// branch of a key before its staging branch.
    snapshot_branches: Vec<SurveyedBranch>,
}

impl TenantSurvey {
    /// Read the ledgers of the database `connection` is attached to, whose
    /// search path is the one `public`'s migrations run under.
    ///
    /// A database no migration has reached yet has no ledger and no
    /// `snapshots` relation, and reads as having applied nothing and holding
    /// no Snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Migration`] when `public`'s ledger cannot
    /// be read, [`ContextStoreError::SnapshotMigration`] naming the branch
    /// when a Snapshot branch's ledger cannot be read,
    /// [`ContextStoreError::StoredSnapshot`] when a control record's key or
    /// URI does not read back, and [`ContextStoreError::Database`] when a
    /// statement fails.
    pub(crate) async fn read(connection: &mut PgConnection) -> Result<TenantSurvey, ContextStoreError> {
        let public_applied = if holds(connection, relation_exists(TENANT_LEDGER)).await? {
            versions(&connection.list_applied_migrations(TENANT_LEDGER).await?).collect()
        } else {
            BTreeSet::new()
        };
        let mut snapshot_branches = Vec::new();
        for branch in snapshot_branches_of(connection).await? {
            let applied = connection.list_applied_migrations(&branch_ledger(branch.key(), branch.branch())).await;
            let applied_through = match applied {
                Ok(applied) => versions(&applied).max(),
                Err(source) => return Err(SnapshotMigrationError::LedgerUnreadable { branch, source }.into()),
            };
            snapshot_branches.push(SurveyedBranch { branch, applied_through });
        }
        Ok(TenantSurvey {
            public_applied,
            snapshot_branches,
        })
    }

    /// The migrations `public`'s ledger records.
    pub(crate) const fn public_applied(&self) -> &BTreeSet<MigrationVersion> {
        &self.public_applied
    }

    /// How far the least advanced Snapshot branch schema has come.
    pub(crate) fn snapshot_progress(&self) -> SnapshotProgress {
        self.snapshot_branches
            .iter()
            .map(|surveyed| surveyed.applied_through)
            .min()
            .map_or(SnapshotProgress::NoSnapshots, SnapshotProgress::LeastAdvanced)
    }

    /// Every Snapshot branch schema, in the order the walk migrates them.
    pub(crate) fn snapshot_branches(&self) -> impl ExactSizeIterator<Item = &SnapshotTarget> {
        self.snapshot_branches.iter().map(|surveyed| &surveyed.branch)
    }
}

/// The versions of the migrations a ledger records.
fn versions(applied: &[AppliedMigration]) -> impl Iterator<Item = MigrationVersion> + '_ {
    applied.iter().map(|migration| MigrationVersion::new(migration.version))
}

/// Every Snapshot branch whose control record and schema both exist.
async fn snapshot_branches_of(connection: &mut PgConnection) -> Result<Vec<SnapshotTarget>, ContextStoreError> {
    if !holds(connection, relation_exists(&Snapshots::Table.to_string())).await? {
        return Ok(Vec::new());
    }
    let mut branches = Vec::new();
    for SnapshotIdentity { key, id } in read_snapshot_identities(&mut *connection).await? {
        for branch in [SnapshotBranch::Published, SnapshotBranch::Staging] {
            if holds(connection, schema_exists(&snapshot_schema_name(key, branch))).await? {
                // clone: each branch schema a Snapshot holds names its URI.
                branches.push(SnapshotTarget::new(key, branch, id.clone()));
            }
        }
    }
    Ok(branches)
}

/// Whether the one-row truth `probe` holds.
async fn holds(connection: &mut PgConnection, probe: SimpleExpr) -> Result<bool, sqlx::Error> {
    Query::select().expr(probe).to_owned().to_query_scalar::<bool>().fetch_one(connection).await
}

#[cfg(test)]
mod tests {
    use crate::{
        lockstep_migration::{
            plan::SnapshotProgress,
            survey::{SurveyedBranch, TenantSurvey},
        },
        migration_set::migration_version::MigrationVersion,
    };
    use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;

    fn surveyed(key: i64, applied_through: Option<i64>) -> SurveyedBranch {
        SurveyedBranch {
            branch: SnapshotTarget::published(
                SnapshotKey::try_from(key).expect("a positive key"),
                "urn:ngsi-ld:Snapshot:surveyed".parse().expect("a valid URI"),
            ),
            applied_through: applied_through.map(MigrationVersion::new),
        }
    }

    fn survey(branches: Vec<SurveyedBranch>) -> TenantSurvey {
        TenantSurvey {
            public_applied: BTreeSet::new(),
            snapshot_branches: branches,
        }
    }

    #[test]
    fn the_snapshots_have_come_as_far_as_the_least_advanced_branch() {
        assert_eq!(survey(Vec::new()).snapshot_progress(), SnapshotProgress::NoSnapshots);
        assert_eq!(
            survey(vec![surveyed(7, Some(33)), surveyed(8, Some(22))]).snapshot_progress(),
            SnapshotProgress::LeastAdvanced(Some(MigrationVersion::new(22)))
        );
        assert_eq!(
            survey(vec![surveyed(7, Some(33)), surveyed(8, None)]).snapshot_progress(),
            SnapshotProgress::LeastAdvanced(None)
        );
    }
}
