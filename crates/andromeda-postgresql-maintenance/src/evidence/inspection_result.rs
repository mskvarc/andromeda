//! What one tenant's inspection came to.

use crate::controller::PassOutcome;
use andromeda_postgresql_codes::maintenance_inspection_result::MaintenanceInspectionResultCode;
use strum::EnumIter;

/// What the last maintenance inspection of a tenant came to.
///
/// One vocabulary for every value `postgresql_maintenance_tenants.last_result`
/// holds: the pass bookkeeping of the controller itself and the outcome of a
/// pass. The pass outcome is folded into it by an exhaustive match, so the
/// stored column has exactly one owner.
#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
pub(crate) enum InspectionResult {
    /// The inspection started and has not finished.
    Started,
    /// The tenant database could not be reached.
    Unavailable,
    /// The controller is disabled.
    Disabled,
    /// Another replica holds the fleet-wide lock.
    GlobalLockUnavailable,
    /// No tenant was due.
    NoTenant,
    /// The selected tenant was unavailable.
    TenantUnavailable,
    /// The tenant schema is behind the binary.
    SchemaBehind,
    /// Nothing needed attention.
    Idle,
    /// A detailed probe ran.
    Probed,
    /// An action was queued.
    Queued,
    /// An action is due but acting is disabled.
    WouldAct,
    /// An action was deferred.
    Deferred,
    /// An action ran.
    Acted,
    /// An action failed.
    ActionFailed,
    /// The pass was cancelled.
    Cancelled,
}

impl InspectionResult {
    /// The result a finished pass records.
    pub(crate) const fn of_pass(outcome: PassOutcome) -> InspectionResult {
        match outcome {
            PassOutcome::Disabled => InspectionResult::Disabled,
            PassOutcome::GlobalLockUnavailable => InspectionResult::GlobalLockUnavailable,
            PassOutcome::NoTenant => InspectionResult::NoTenant,
            PassOutcome::TenantUnavailable => InspectionResult::TenantUnavailable,
            PassOutcome::SchemaBehind => InspectionResult::SchemaBehind,
            PassOutcome::Idle => InspectionResult::Idle,
            PassOutcome::Probed => InspectionResult::Probed,
            PassOutcome::Queued => InspectionResult::Queued,
            PassOutcome::WouldAct => InspectionResult::WouldAct,
            PassOutcome::Deferred => InspectionResult::Deferred,
            PassOutcome::Acted => InspectionResult::Acted,
            PassOutcome::ActionFailed => InspectionResult::ActionFailed,
            PassOutcome::Cancelled => InspectionResult::Cancelled,
        }
    }

    /// The registry member the platform database stores for this result.
    pub(crate) const fn code(self) -> MaintenanceInspectionResultCode {
        match self {
            InspectionResult::Started => MaintenanceInspectionResultCode::Started,
            InspectionResult::Unavailable => MaintenanceInspectionResultCode::Unavailable,
            InspectionResult::Disabled => MaintenanceInspectionResultCode::Disabled,
            InspectionResult::GlobalLockUnavailable => MaintenanceInspectionResultCode::GlobalLockUnavailable,
            InspectionResult::NoTenant => MaintenanceInspectionResultCode::NoTenant,
            InspectionResult::TenantUnavailable => MaintenanceInspectionResultCode::TenantUnavailable,
            InspectionResult::SchemaBehind => MaintenanceInspectionResultCode::SchemaBehind,
            InspectionResult::Idle => MaintenanceInspectionResultCode::Idle,
            InspectionResult::Probed => MaintenanceInspectionResultCode::Probed,
            InspectionResult::Queued => MaintenanceInspectionResultCode::Queued,
            InspectionResult::WouldAct => MaintenanceInspectionResultCode::WouldAct,
            InspectionResult::Deferred => MaintenanceInspectionResultCode::Deferred,
            InspectionResult::Acted => MaintenanceInspectionResultCode::Acted,
            InspectionResult::ActionFailed => MaintenanceInspectionResultCode::ActionFailed,
            InspectionResult::Cancelled => MaintenanceInspectionResultCode::Cancelled,
        }
    }

    /// Whether this result ends a pass rather than marking its start, which is
    /// what resets the tenant's consecutive-failure count.
    pub(crate) const fn concludes_the_pass(self) -> bool {
        !matches!(self, InspectionResult::Started)
    }
}

#[cfg(test)]
mod tests {
    use crate::evidence::inspection_result::InspectionResult;
    use andromeda_postgresql_codes::maintenance_inspection_result::MaintenanceInspectionResultCode;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    /// The mapping is one-to-one, and onto every number but the capability
    /// ones: a missing `MAINTAIN` privilege leaves a visit observing, and a
    /// missing `pgstattuple` leaves it without index probes, so either visit
    /// records its own outcome and those numbers are only ever decoded.
    #[test]
    fn every_inspection_result_has_its_own_stored_number_and_only_the_capability_numbers_are_unwritten() {
        let unwritten = [
            MaintenanceInspectionResultCode::Ready,
            MaintenanceInspectionResultCode::MissingPgstattuple,
            MaintenanceInspectionResultCode::MissingPgstattupleExecute,
            MaintenanceInspectionResultCode::MissingMaintain,
            MaintenanceInspectionResultCode::CapabilityUnavailable,
        ];
        let stored: Vec<MaintenanceInspectionResultCode> = InspectionResult::iter().map(InspectionResult::code).collect();
        let written: Vec<MaintenanceInspectionResultCode> = MaintenanceInspectionResultCode::ALL
            .iter()
            .copied()
            .filter(|code| !unwritten.contains(code))
            .collect();
        assert_eq!(stored, written);
    }

    #[test]
    fn only_the_start_marker_leaves_the_failure_count_alone() {
        let non_concluding: Vec<InspectionResult> = InspectionResult::iter().filter(|result| !result.concludes_the_pass()).collect();
        assert_eq!(non_concluding, vec![InspectionResult::Started]);
    }
}
