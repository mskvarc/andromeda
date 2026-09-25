//! How one maintenance action ended.

use andromeda_postgresql_codes::maintenance_action_result::MaintenanceActionResultCode;
use strum::EnumIter;

/// How the last maintenance action on a target ended.
#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
pub(crate) enum ActionResult {
    /// The statement completed.
    Succeeded,
    /// The statement failed; the action stays pending for a later retry.
    Failed,
}

impl ActionResult {
    /// The registry member the platform database stores for this result.
    pub(crate) const fn code(self) -> MaintenanceActionResultCode {
        match self {
            ActionResult::Succeeded => MaintenanceActionResultCode::Succeeded,
            ActionResult::Failed => MaintenanceActionResultCode::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::evidence::action_result::ActionResult;
    use andromeda_postgresql_codes::maintenance_action_result::MaintenanceActionResultCode;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    /// The mapping is one-to-one and onto: every result has its own number and
    /// every number the registry assigns is some result's.
    #[test]
    fn every_action_result_has_its_own_stored_number_and_every_number_is_used() {
        let stored: Vec<MaintenanceActionResultCode> = ActionResult::iter().map(ActionResult::code).collect();
        assert_eq!(stored, MaintenanceActionResultCode::ALL.to_vec());
    }

    #[test]
    fn the_stored_label_names_the_result() {
        assert_eq!(ActionResult::Succeeded.code().label(), "succeeded");
        assert_eq!(ActionResult::Failed.code().label(), "failed");
    }
}
