//! Configuration selecting whether and how PostgreSQL maintenance runs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Whether the maintenance controller is active and may mutate tenant databases.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, Eq, PartialEq, JsonSchema)]
#[schemars(description = "Whether the maintenance controller runs, and whether it may change tenant databases.")]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum MaintenanceMode {
    /// Do not register or execute the controller.
    #[default]
    #[schemars(description = "Do not run the controller.")]
    Disabled,
    /// Inspect and report decisions without executing maintenance statements.
    #[schemars(description = "Inspect the databases and report the maintenance the controller decides on, without carrying any of it out.")]
    Observe,
    /// Inspect and execute qualifying maintenance statements.
    #[schemars(description = "Inspect the databases and carry out the maintenance the controller decides on.")]
    Automatic,
}

/// A fixed maintenance policy profile.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, Eq, PartialEq, JsonSchema)]
#[schemars(description = "The fixed thresholds and cadence the maintenance controller follows.")]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum MaintenanceProfile {
    /// Prefer fewer probes and a high confidence threshold before acting.
    #[default]
    #[schemars(description = "Inspect less often and require stronger evidence before acting.")]
    Conservative,
    /// Inspect more frequently and react to smaller degradation signals.
    #[schemars(description = "Inspect more often and react to smaller signs of degradation.")]
    Responsive,
}

/// Adaptive PostgreSQL maintenance configuration.
#[derive(Clone, Copy, Deserialize, Serialize, Default, JsonSchema, Debug, Eq, PartialEq)]
#[schemars(description = "Adaptive PostgreSQL inspection and maintenance of the tenant databases.")]
#[schemars(deny_unknown_fields)]
pub struct MaintenanceConfig {
    /// Whether inspection is disabled, observational, or automatic.
    #[schemars(description = "Whether maintenance is off, only inspects and reports, or also acts on what it finds.")]
    pub mode: MaintenanceMode,
    /// Fixed threshold and cadence profile.
    #[schemars(description = "How often the controller inspects, and how much evidence it needs before it acts.")]
    pub profile: MaintenanceProfile,
}

#[cfg(test)]
mod tests {
    use crate::schema::database::maintenance::{MaintenanceConfig, MaintenanceMode, MaintenanceProfile};
    use pretty_assertions::assert_eq;

    #[test]
    fn omitted_maintenance_configuration_is_disabled_and_conservative() {
        let config = MaintenanceConfig::default();
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
