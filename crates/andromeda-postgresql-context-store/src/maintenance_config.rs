//! Configuration selecting whether and how PostgreSQL maintenance runs.

use confique::Config;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Whether the maintenance controller is active and may mutate tenant databases.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum MaintenanceMode {
    /// Do not register or execute the controller.
    #[default]
    Disabled,
    /// Inspect and report decisions without executing maintenance statements.
    Observe,
    /// Inspect and execute qualifying maintenance statements.
    Automatic,
}

/// A fixed maintenance policy profile.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum MaintenanceProfile {
    /// Prefer fewer probes and a high confidence threshold before acting.
    #[default]
    Conservative,
    /// Inspect more frequently and react to smaller degradation signals.
    Responsive,
}

/// Adaptive PostgreSQL maintenance configuration.
#[derive(Clone, Copy, Config, Debug, Default, Serialize, Eq, PartialEq)]
pub struct MaintenanceConfig {
    /// Whether inspection is disabled, observational, or automatic.
    #[config(env = "ANDROMEDA_DATABASE_MAINTENANCE_MODE", default = "disabled")]
    pub mode: MaintenanceMode,
    /// Fixed threshold and cadence profile.
    #[config(env = "ANDROMEDA_DATABASE_MAINTENANCE_PROFILE", default = "conservative")]
    pub profile: MaintenanceProfile,
}

#[cfg(test)]
mod tests {
    use crate::maintenance_config::{MaintenanceConfig, MaintenanceMode, MaintenanceProfile};
    use confique::Config;

    #[test]
    fn omitted_maintenance_configuration_is_disabled_and_conservative() {
        let config = MaintenanceConfig::builder().load().expect("built-in defaults must load");
        assert_eq!(config.mode, MaintenanceMode::Disabled);
        assert_eq!(config.profile, MaintenanceProfile::Conservative);
    }

    #[test]
    fn all_modes_and_profiles_parse_as_kebab_case() {
        for raw in ["disabled", "observe", "automatic"] {
            assert!(raw.parse::<MaintenanceMode>().is_ok(), "{raw}");
        }
        for raw in ["conservative", "responsive"] {
            assert!(raw.parse::<MaintenanceProfile>().is_ok(), "{raw}");
        }
        assert!("auto".parse::<MaintenanceMode>().is_err());
        assert!("fast".parse::<MaintenanceProfile>().is_err());
    }
}
