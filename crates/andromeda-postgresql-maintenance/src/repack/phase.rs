//! The phases one repack runs through.

use andromeda_postgresql_codes::maintenance_repack_phase::MaintenanceRepackPhaseCode;
use andromeda_telemetry::maintenance::RepackPhaseLabel;

/// One phase of a repack, in the order a run reaches them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::Display, strum::EnumIter)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum RepackPhase {
    /// The log table and the trigger filling it are created.
    Setup,
    /// The table's rows are copied.
    Copy,
    /// The copy's indexes are built.
    IndexBuild,
    /// The writes logged since the copy are replayed until the log has
    /// converged and every transaction older than the copy has ended.
    Replay,
    /// The table is given the copy's files.
    Swap,
    /// The run's objects, the old files among them, are dropped.
    Drop,
    /// The repacked table's planner statistics are refreshed.
    Analyze,
}

impl RepackPhase {
    /// The code the run row stores, `None` for the analyze, which runs after
    /// the row is gone.
    pub(crate) const fn code(self) -> Option<MaintenanceRepackPhaseCode> {
        match self {
            RepackPhase::Setup => Some(MaintenanceRepackPhaseCode::Setup),
            RepackPhase::Copy => Some(MaintenanceRepackPhaseCode::Copy),
            RepackPhase::IndexBuild => Some(MaintenanceRepackPhaseCode::IndexBuild),
            RepackPhase::Replay => Some(MaintenanceRepackPhaseCode::Replay),
            RepackPhase::Swap => Some(MaintenanceRepackPhaseCode::Swap),
            RepackPhase::Drop => Some(MaintenanceRepackPhaseCode::Drop),
            RepackPhase::Analyze => None,
        }
    }

    /// The phase a stored code names.
    pub(crate) const fn of_code(code: MaintenanceRepackPhaseCode) -> RepackPhase {
        match code {
            MaintenanceRepackPhaseCode::Setup => RepackPhase::Setup,
            MaintenanceRepackPhaseCode::Copy => RepackPhase::Copy,
            MaintenanceRepackPhaseCode::IndexBuild => RepackPhase::IndexBuild,
            MaintenanceRepackPhaseCode::Replay => RepackPhase::Replay,
            MaintenanceRepackPhaseCode::Swap => RepackPhase::Swap,
            MaintenanceRepackPhaseCode::Drop => RepackPhase::Drop,
        }
    }

    /// The phase's metric label.
    pub(crate) const fn label(self) -> RepackPhaseLabel {
        match self {
            RepackPhase::Setup => RepackPhaseLabel::Setup,
            RepackPhase::Copy => RepackPhaseLabel::Copy,
            RepackPhase::IndexBuild => RepackPhaseLabel::IndexBuild,
            RepackPhase::Replay => RepackPhaseLabel::Replay,
            RepackPhase::Swap => RepackPhaseLabel::Swap,
            RepackPhase::Drop => RepackPhaseLabel::Drop,
            RepackPhase::Analyze => RepackPhaseLabel::Analyze,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::repack::phase::RepackPhase;
    use andromeda_postgresql_codes::maintenance_repack_phase::MaintenanceRepackPhaseCode;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_stored_phase_round_trips_and_only_the_analyze_is_unstored() {
        let stored: Vec<MaintenanceRepackPhaseCode> = RepackPhase::iter().filter_map(RepackPhase::code).collect();
        assert_eq!(stored, MaintenanceRepackPhaseCode::ALL.to_vec());
        for code in MaintenanceRepackPhaseCode::ALL {
            assert_eq!(RepackPhase::of_code(*code).code(), Some(*code));
        }
        assert_eq!(RepackPhase::Analyze.code(), None);
    }

    #[test]
    fn each_phase_is_labelled_as_its_metric_names_it() {
        let labels: Vec<&'static str> = RepackPhase::iter().map(|phase| phase.label().into()).collect();
        let names: Vec<String> = RepackPhase::iter().map(|phase| phase.to_string()).collect();
        assert_eq!(labels, names);
    }
}
