//! The branches one visit works in and the mode each is decided under.
//!
//! Each mode does everything the mode below it does. Automatic mode acts in a
//! branch only where the role may maintain every heap of that branch it may
//! vacuum, and samples, probes and reports every other branch the way
//! observe mode does: a missing privilege takes away acting, never sampling.
//! Observe mode checks no privilege.

use crate::{
    branch::Branch,
    capability::{self, MaintainPrivilege},
    error::MaintenanceError,
    state::RegisteredTenant,
};
use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
use sqlx::PgConnection;
use tracing::warn;

/// One branch a visit works in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VisitBranch {
    /// The branch.
    pub(crate) branch: Branch,
    /// The mode every decision on a target of the branch is taken under.
    pub(crate) mode: MaintenanceMode,
}

/// The branches a visit works in, the live branch first, each with its mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VisitBranches {
    /// The branches in the order the visit samples them.
    branches: Vec<VisitBranch>,
}

impl VisitBranches {
    /// Every branch with its mode, in sampling order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = VisitBranch> + '_ {
        self.branches.iter().copied()
    }

    /// Every branch, in sampling order.
    pub(crate) fn branches(&self) -> impl Iterator<Item = Branch> + '_ {
        self.iter().map(|visit| visit.branch)
    }

    /// The branches decided under automatic mode, in sampling order.
    pub(crate) fn acting(&self) -> impl Iterator<Item = Branch> + '_ {
        self.iter().filter(|visit| visit.mode == MaintenanceMode::Automatic).map(|visit| visit.branch)
    }

    /// The branches decided under any other mode, in sampling order.
    pub(crate) fn observed(&self) -> impl Iterator<Item = Branch> + '_ {
        self.iter().filter(|visit| visit.mode != MaintenanceMode::Automatic).map(|visit| visit.branch)
    }

    /// Whether the visit works in `branch`.
    pub(crate) fn contains(&self, branch: Branch) -> bool {
        self.branches().any(|visited| visited == branch)
    }

    /// The mode a target of `branch` is decided under. A branch outside the
    /// visit is observed: the visit checked no privilege there.
    pub(crate) fn mode(&self, branch: Branch) -> MaintenanceMode {
        self.iter()
            .find(|visit| visit.branch == branch)
            .map_or(MaintenanceMode::Observe, |visit| visit.mode)
    }
}

/// The mode a branch is decided under, given the configured mode and whether
/// the role may maintain the branch.
pub(crate) const fn branch_mode(configured: MaintenanceMode, privilege: MaintainPrivilege) -> MaintenanceMode {
    match (configured, privilege) {
        (MaintenanceMode::Automatic, MaintainPrivilege::Held) => MaintenanceMode::Automatic,
        (MaintenanceMode::Automatic, MaintainPrivilege::NotGranted) => MaintenanceMode::Observe,
        (MaintenanceMode::Disabled | MaintenanceMode::Observe, MaintainPrivilege::Held | MaintainPrivilege::NotGranted) => configured,
    }
}

/// Decide the mode of every branch in `branches`, in order, under
/// `configured`, and log the branches automatic mode only observes.
pub(crate) async fn decide_branches(
    configured: MaintenanceMode,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branches: &[Branch],
) -> Result<VisitBranches, MaintenanceError> {
    let branches = match configured {
        MaintenanceMode::Disabled | MaintenanceMode::Observe => branches.iter().map(|&branch| VisitBranch { branch, mode: configured }).collect(),
        MaintenanceMode::Automatic => {
            let privileges = capability::maintain_privileges(connection, branches).await?;
            report_observed_branches(tenant, &privileges);
            privileges
                .into_iter()
                .map(|(branch, privilege)| VisitBranch {
                    branch,
                    mode: branch_mode(configured, privilege),
                })
                .collect()
        }
    };
    Ok(VisitBranches { branches })
}

/// Log, once per visit, every branch automatic mode only observes because the
/// role may not maintain it.
fn report_observed_branches(tenant: &RegisteredTenant, privileges: &[(Branch, MaintainPrivilege)]) {
    let not_granted = not_granted_branches(privileges);
    if !not_granted.is_empty() {
        warn!(tenant = tenant.log_name(), database = %tenant.database, branches = ?not_granted, decision = "capability-unavailable", reason = "missing-maintain", "PostgreSQL maintenance only observes the branches whose heaps the role may not maintain (MAINTAIN, or ownership before PostgreSQL 17)");
    }
}

/// The branches of `privileges` the role may not maintain.
fn not_granted_branches(privileges: &[(Branch, MaintainPrivilege)]) -> Vec<Branch> {
    privileges
        .iter()
        .filter(|&&(_, privilege)| privilege == MaintainPrivilege::NotGranted)
        .map(|&(branch, _)| branch)
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::Branch,
        capability::MaintainPrivilege,
        controller::visit_branches::{VisitBranch, VisitBranches, branch_mode, not_granted_branches},
    };
    use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use pretty_assertions::assert_eq;

    fn snapshot(raw: i64) -> Branch {
        Branch::Snapshot(SnapshotKey::try_from(raw).expect("positive snapshot key"))
    }

    #[test]
    fn automatic_mode_acts_only_in_a_branch_whose_heaps_the_role_may_maintain() {
        assert_eq!(branch_mode(MaintenanceMode::Automatic, MaintainPrivilege::Held), MaintenanceMode::Automatic);
        assert_eq!(branch_mode(MaintenanceMode::Automatic, MaintainPrivilege::NotGranted), MaintenanceMode::Observe);
    }

    #[test]
    fn a_privilege_never_lifts_observe_mode_into_acting() {
        for privilege in [MaintainPrivilege::Held, MaintainPrivilege::NotGranted] {
            assert_eq!(branch_mode(MaintenanceMode::Observe, privilege), MaintenanceMode::Observe, "{privilege:?}");
            assert_eq!(branch_mode(MaintenanceMode::Disabled, privilege), MaintenanceMode::Disabled, "{privilege:?}");
        }
    }

    #[test]
    fn each_branch_keeps_its_own_mode_and_an_unvisited_branch_is_observed() {
        let branches = VisitBranches {
            branches: vec![
                VisitBranch {
                    branch: Branch::Live,
                    mode: MaintenanceMode::Automatic,
                },
                VisitBranch {
                    branch: snapshot(7),
                    mode: MaintenanceMode::Observe,
                },
            ],
        };
        assert_eq!(branches.mode(Branch::Live), MaintenanceMode::Automatic);
        assert_eq!(branches.mode(snapshot(7)), MaintenanceMode::Observe);
        assert_eq!(branches.mode(snapshot(8)), MaintenanceMode::Observe);
        assert!(branches.contains(snapshot(7)));
        assert!(!branches.contains(snapshot(8)));
        assert_eq!(branches.branches().collect::<Vec<_>>(), [Branch::Live, snapshot(7)]);
        assert_eq!(branches.acting().collect::<Vec<_>>(), [Branch::Live]);
        assert_eq!(branches.observed().collect::<Vec<_>>(), [snapshot(7)]);
    }

    #[test]
    fn the_missing_grant_warning_names_every_branch_without_the_grant() {
        let privileges = [
            (Branch::Live, MaintainPrivilege::NotGranted),
            (snapshot(3), MaintainPrivilege::Held),
            (snapshot(5), MaintainPrivilege::NotGranted),
        ];
        assert_eq!(not_granted_branches(&privileges), [Branch::Live, snapshot(5)]);
    }
}
