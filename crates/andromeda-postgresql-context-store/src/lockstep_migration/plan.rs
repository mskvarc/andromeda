//! The steps that bring every schema of a tenant database through the tenant
//! set, decided from what the ledgers record.

use crate::migration_set::{
    migration_version::MigrationVersion,
    tenant::{MigrationReach, TenantSetMigration},
};
use std::collections::BTreeSet;

/// One step of the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockstepStep {
    /// Bring `public` through this migration of the tenant set.
    Public(MigrationVersion),
    /// Bring every Snapshot branch schema through this branch migration.
    Snapshots(MigrationVersion),
}

/// How far the Snapshot branch schemas of a tenant database have come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotProgress {
    /// The database holds no Snapshot branch schema.
    NoSnapshots,
    /// The highest migration the least advanced Snapshot branch schema has
    /// applied, `None` when one has applied none.
    LeastAdvanced(Option<MigrationVersion>),
}

/// The steps that bring `public` and every Snapshot branch schema through
/// `migrations`, given the migrations `public` has applied and how far the
/// Snapshot branches have come.
///
/// The Snapshot branches first catch up with the branch migrations `public`
/// has applied. A Snapshot branch is behind a branch migration `public` has
/// applied only when a walk stopped between the two. The migrations `public`
/// lacks then run in version order, and `public` stops at each branch
/// migration until every Snapshot branch has applied it too. The walk ends
/// with `public` brought through the last migration, which checks the
/// migrations its ledger records against the embedded ones even when nothing
/// is left to apply.
pub(crate) fn lockstep_plan(migrations: &[TenantSetMigration], public_applied: &BTreeSet<MigrationVersion>, snapshots: SnapshotProgress) -> Vec<LockstepStep> {
    let branch_migrations = || migrations.iter().filter(|migration| migration.reach == MigrationReach::EveryBranch);
    let mut steps = Vec::new();
    if let SnapshotProgress::LeastAdvanced(least) = snapshots {
        let reached = branch_migrations()
            .map(|migration| migration.version)
            .filter(|version| public_applied.contains(version))
            .max();
        if let Some(reached) = reached
            && least < Some(reached)
        {
            steps.push(LockstepStep::Snapshots(reached));
        }
        for pending in branch_migrations().filter(|migration| !public_applied.contains(&migration.version)) {
            steps.push(LockstepStep::Public(pending.version));
            steps.push(LockstepStep::Snapshots(pending.version));
        }
    }
    if let Some(last) = migrations.iter().map(|migration| migration.version).max()
        && !steps.contains(&LockstepStep::Public(last))
    {
        steps.push(LockstepStep::Public(last));
    }
    steps
}

#[cfg(test)]
mod tests {
    use crate::{
        lockstep_migration::plan::{
            LockstepStep::{Public, Snapshots},
            SnapshotProgress,
            lockstep_plan,
        },
        migration_set::{
            migration_version::MigrationVersion,
            tenant::{MigrationReach, TenantSetMigration},
        },
    };
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;

    /// A tenant set shaped like the embedded one: a run of branch migrations
    /// among the first tenant files, a version the platform set takes, and a
    /// later branch migration between two tenant files.
    fn tenant_set() -> Vec<TenantSetMigration> {
        (5..=36)
            .filter(|version| *version != 30)
            .map(|version| TenantSetMigration {
                version: MigrationVersion::new(version),
                reach: if (15..=22).contains(&version) || version == 33 {
                    MigrationReach::EveryBranch
                } else {
                    MigrationReach::Public
                },
            })
            .collect()
    }

    /// The migrations of the tenant set numbered at most `last`, as `public`
    /// records them once a run brought it through `last`.
    fn applied_through(last: i64) -> BTreeSet<MigrationVersion> {
        tenant_set()
            .into_iter()
            .map(|migration| migration.version)
            .filter(|version| version.get() <= last)
            .collect()
    }

    const fn at(version: i64) -> MigrationVersion {
        MigrationVersion::new(version)
    }

    const fn snapshots_at(version: i64) -> SnapshotProgress {
        SnapshotProgress::LeastAdvanced(Some(MigrationVersion::new(version)))
    }

    #[test]
    fn a_release_tenant_stops_public_at_the_pending_branch_migration_until_every_snapshot_has_it() {
        assert_eq!(
            lockstep_plan(&tenant_set(), &applied_through(29), snapshots_at(22)),
            [Public(at(33)), Snapshots(at(33)), Public(at(36))]
        );
    }

    /// A walk that stopped after `public` applied a branch migration resumes
    /// with the Snapshot branches, before the next tenant migration changes
    /// the types they still need.
    #[test]
    fn snapshots_behind_a_branch_migration_public_applied_catch_up_before_public_moves_on() {
        assert_eq!(
            lockstep_plan(&tenant_set(), &applied_through(33), snapshots_at(22)),
            [Snapshots(at(33)), Public(at(36))]
        );
    }

    #[test]
    fn snapshots_behind_a_fully_migrated_public_only_catch_up() {
        assert_eq!(
            lockstep_plan(&tenant_set(), &applied_through(36), snapshots_at(22)),
            [Snapshots(at(33)), Public(at(36))]
        );
    }

    #[test]
    fn a_snapshot_whose_ledger_is_empty_catches_up_with_every_branch_migration_public_applied() {
        assert_eq!(
            lockstep_plan(&tenant_set(), &applied_through(36), SnapshotProgress::LeastAdvanced(None)),
            [Snapshots(at(33)), Public(at(36))]
        );
    }

    /// A Snapshot ahead of `public` has nothing to catch up with.
    #[test]
    fn snapshots_ahead_of_public_only_see_public_move() {
        assert_eq!(lockstep_plan(&tenant_set(), &applied_through(33), snapshots_at(36)), [Public(at(36))]);
    }

    #[test]
    fn every_pending_branch_migration_stops_public_until_the_snapshots_have_it() {
        let barriers = (15..=22).chain([33]).flat_map(|version| [Public(at(version)), Snapshots(at(version))]);
        let expected: Vec<_> = barriers.chain([Public(at(36))]).collect();
        assert_eq!(
            lockstep_plan(&tenant_set(), &applied_through(14), SnapshotProgress::LeastAdvanced(None)),
            expected
        );
    }

    #[test]
    fn with_no_branch_migration_pending_only_public_moves() {
        assert_eq!(lockstep_plan(&tenant_set(), &applied_through(33), snapshots_at(33)), [Public(at(36))]);
    }

    /// Bringing `public` through its last migration applies nothing then,
    /// and checks every migration its ledger records.
    #[test]
    fn with_nothing_pending_public_is_still_checked_against_the_embedded_set() {
        assert_eq!(lockstep_plan(&tenant_set(), &applied_through(36), snapshots_at(33)), [Public(at(36))]);
    }

    #[test]
    fn without_snapshots_public_runs_the_whole_tenant_set_at_once() {
        assert_eq!(lockstep_plan(&tenant_set(), &BTreeSet::new(), SnapshotProgress::NoSnapshots), [Public(at(36))]);
        assert_eq!(
            lockstep_plan(&tenant_set(), &applied_through(29), SnapshotProgress::NoSnapshots),
            [Public(at(36))]
        );
    }

    /// The Snapshots catch up with the last migration, and `public`, which
    /// already holds it, is still checked against the embedded set.
    #[test]
    fn snapshots_behind_an_applied_last_branch_migration_catch_up_before_public_is_checked() {
        let ending_with_a_branch_migration: Vec<TenantSetMigration> = tenant_set().into_iter().filter(|migration| migration.version <= at(33)).collect();
        assert_eq!(
            lockstep_plan(&ending_with_a_branch_migration, &applied_through(33), snapshots_at(22)),
            [Snapshots(at(33)), Public(at(33))]
        );
    }

    #[test]
    fn a_pending_last_migration_of_the_branch_set_ends_the_walk_in_the_snapshots() {
        let ending_with_a_branch_migration: Vec<TenantSetMigration> = tenant_set().into_iter().filter(|migration| migration.version <= at(33)).collect();
        assert_eq!(
            lockstep_plan(&ending_with_a_branch_migration, &applied_through(32), snapshots_at(22)),
            [Public(at(33)), Snapshots(at(33))]
        );
    }
}
