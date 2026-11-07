//! The rights a visit's actions need beyond `MAINTAIN` on a branch's heaps.
//!
//! Recompressing a partial history chunk needs ownership of its hypertable,
//! rebuilding a closed rowstore chunk's indexes needs `MAINTAIN` on the chunk
//! (ownership before PostgreSQL 17), and dropping a leftover index needs
//! ownership of the table it indexes. Each right is checked only where
//! automatic mode acts and there is work for the action, on the relation the
//! action would work on. A missing right takes away that one action: it is
//! decided the way observe mode decides it, an action queued while the right
//! was held is withdrawn, and the visit goes on to everything else.

use crate::{
    artifact::RecoveryArtifact,
    branch::BranchTarget,
    candidate::MaintenanceTarget,
    capability::{self, MaintainPrivilege, MaintainRight},
    columnstore_partiality::ColumnstorePartiality,
    controller::visit_branches::{VisitBranch, branch_mode},
    error::MaintenanceError,
    policy::PolicyProfile,
    rowstore_rebuild::{self, RowstoreRebuild},
    state::{RegisteredTenant, pending_action},
};
use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
use sqlx::PgConnection;
use tracing::{info, warn};

/// The targets of a visit whose action the role lacks the right to run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ActionRights {
    /// The targets, each in its branch, whose right is missing.
    missing: Vec<BranchTarget>,
}

impl ActionRights {
    /// Whether the role may run the action of `row`. A target whose right was
    /// never checked needs none beyond its branch's.
    pub(crate) fn privilege(&self, row: BranchTarget) -> MaintainPrivilege {
        if self.missing.contains(&row) {
            MaintainPrivilege::NotGranted
        } else {
            MaintainPrivilege::Held
        }
    }

    /// The mode the action of `row` is decided under in a branch decided
    /// under `branch_mode`.
    pub(crate) fn mode(&self, row: BranchTarget, branch: MaintenanceMode) -> MaintenanceMode {
        branch_mode(branch, self.privilege(row))
    }

    /// Add the targets of `other`.
    pub(crate) fn extend(&mut self, other: ActionRights) {
        self.missing.extend(other.missing);
    }
}

/// Check, in a visited branch automatic mode acts in, the right of every history action
/// with work to do: the recompression of a hypertable with a partial chunk and
/// the rebuild of the oldest closed rowstore chunk awaiting one. Every missing
/// right is logged and its queued action withdrawn.
pub(crate) async fn history_rights(
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    VisitBranch { branch, mode }: VisitBranch,
    partiality: &[ColumnstorePartiality],
    rebuilds: &[RowstoreRebuild],
    profile: PolicyProfile,
) -> Result<ActionRights, MaintenanceError> {
    let mut rights = ActionRights::default();
    if mode != MaintenanceMode::Automatic {
        return Ok(rights);
    }
    for sample in partiality.iter().filter(|sample| sample.partial_chunks > 0) {
        let relation = branch.qualified(sample.table.relation_name());
        if capability::ownership(connection, &relation).await? == MaintainPrivilege::NotGranted {
            let row = BranchTarget::new(branch, MaintenanceTarget::Columnstore(sample.table))?;
            deny(platform, tenant, row, MaintainRight::Ownership, &relation).await?;
            rights.missing.push(row);
        }
    }
    let awaiting = rebuilds.iter().filter(|sample| sample.awaiting_chunks > 0).collect::<Vec<_>>();
    if awaiting.is_empty() {
        return Ok(rights);
    }
    let server = capability::server_version(connection).await?;
    for sample in awaiting {
        let Some((chunk, _)) = rowstore_rebuild::oldest_awaiting_chunk(connection, sample.table, profile).await? else {
            continue;
        };
        let relation = chunk.chunk.relation;
        if capability::relation_maintain(connection, server, &relation).await? == MaintainPrivilege::NotGranted {
            let row = BranchTarget::new(branch, MaintenanceTarget::Rowstore(sample.table))?;
            deny(platform, tenant, row, capability::maintain_right(server), &relation).await?;
            rights.missing.push(row);
        }
    }
    Ok(rights)
}

/// Log the missing `right` on `relation` the action of `row` needs, and
/// withdraw the action if it is queued.
async fn deny(platform: &mut PgConnection, tenant: &RegisteredTenant, row: BranchTarget, right: MaintainRight, relation: &str) -> Result<(), MaintenanceError> {
    let reason: &'static str = right.into();
    let branch = row.branch();
    let target = row.target().relation_name();
    warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target, relation, decision = "capability-unavailable", reason, "PostgreSQL maintenance only observes an action whose right the role lacks");
    for withdrawn in pending_action::withdraw_action(platform, tenant, row).await? {
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = withdrawn.relation_name(), decision = "would-act", "PostgreSQL maintenance withdrew a queued action whose right the role lacks");
    }
    Ok(())
}

/// Whether the role may drop `artifact`: it owns the table the artifact
/// indexes.
pub(crate) async fn artifact_drop_privilege(connection: &mut PgConnection, artifact: &RecoveryArtifact) -> Result<MaintainPrivilege, MaintenanceError> {
    capability::ownership(connection, &artifact.name.table()).await
}

/// Log `artifact`, which the role may not drop, as observe mode logs it, and
/// the ownership it lacks.
pub(crate) fn report_undroppable_artifact(tenant: &RegisteredTenant, artifact: &RecoveryArtifact) {
    let reason: &'static str = MaintainRight::Ownership.into();
    let branch = artifact.name.branch();
    let target = artifact.name.as_str();
    let relation = artifact.name.table();
    warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target, relation = %relation, decision = "capability-unavailable", reason, "PostgreSQL maintenance only observes a leftover index the role may not drop");
    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target, artifact_kind = ?artifact.kind, decision = "would-drop-artifact", "PostgreSQL maintenance recovery observed");
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::{Branch, BranchTarget},
        candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable},
        capability::MaintainPrivilege,
        controller::action_rights::ActionRights,
    };
    use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
    use pretty_assertions::assert_eq;

    fn row(target: MaintenanceTarget) -> BranchTarget {
        BranchTarget::new(Branch::Live, target).expect("the live branch holds every target")
    }

    #[test]
    fn an_action_without_its_right_is_observed_and_every_other_action_still_acts() {
        let recompression = row(MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory));
        let rebuild = row(MaintenanceTarget::Rowstore(HistoryTable::AttributeHistory));
        let vacuum = row(MaintenanceTarget::Heap(HeapTarget::CurrentAttributes));
        let rights = ActionRights { missing: vec![recompression] };
        assert_eq!(rights.privilege(recompression), MaintainPrivilege::NotGranted);
        assert_eq!(rights.mode(recompression, MaintenanceMode::Automatic), MaintenanceMode::Observe);
        for acting in [rebuild, vacuum] {
            assert_eq!(rights.privilege(acting), MaintainPrivilege::Held, "{acting:?}");
            assert_eq!(rights.mode(acting, MaintenanceMode::Automatic), MaintenanceMode::Automatic, "{acting:?}");
        }
    }

    #[test]
    fn a_held_right_never_lifts_an_observed_branch_into_acting() {
        let rights = ActionRights::default();
        let recompression = row(MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory));
        for mode in [MaintenanceMode::Observe, MaintenanceMode::Disabled] {
            assert_eq!(rights.mode(recompression, mode), mode, "{mode:?}");
        }
    }

    #[test]
    fn rights_gathered_across_branches_keep_every_missing_one() {
        let recompression = row(MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory));
        let rebuild = row(MaintenanceTarget::Rowstore(HistoryTable::AttributeHistory));
        let mut rights = ActionRights { missing: vec![recompression] };
        rights.extend(ActionRights { missing: vec![rebuild] });
        assert_eq!(rights.privilege(recompression), MaintainPrivilege::NotGranted);
        assert_eq!(rights.privilege(rebuild), MaintainPrivilege::NotGranted);
    }
}
