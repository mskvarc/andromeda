//! Configuration selecting whether and how PostgreSQL maintenance runs.

use crate::{human_byte_size::HumanByteSize, schema::ceiling::Ceiling};
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
    #[schemars(description = "Do not run the controller.")]
    Disabled,
    /// Inspect and report decisions without executing maintenance statements.
    #[schemars(description = "Inspect the databases and report the maintenance the controller decides on, without carrying any of it out.")]
    Observe,
    /// Inspect and execute qualifying maintenance statements.
    #[default]
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

/// Whether and on which tables the controller repacks online.
///
/// A repack copies a table and its indexes into new files while the table
/// stays in use, then swaps them in, so the database needs free disk of up to
/// twice the table's size while it runs. SQL cannot read filesystem free
/// space, so the default only reports the repacks the controller would run.
#[derive(Clone, Copy, Deserialize, Serialize, JsonSchema, Debug, Eq, PartialEq)]
#[schemars(description = "Online repacks of bloated tables, which rewrite a table and its indexes while it stays in use.")]
#[schemars(deny_unknown_fields)]
pub struct RepackConfig {
    /// Whether repacks are off, only reported, or run. The stricter of this
    /// and [`MaintenanceConfig::mode`] applies.
    #[schemars(
        description = "Whether the controller only reports the repacks it would run, runs them, or neither. The stricter of this and `database.maintenance.mode` applies. A repack copies the table online, so the database needs free disk of up to twice the table's size, which the broker cannot measure. Repacks are only reported by default, so their decisions can be checked against the free disk before they run."
    )]
    pub mode: MaintenanceMode,
    /// The largest table, heap plus TOAST, the controller repacks, in
    /// human-readable form (`"4 GiB"`), or `"unlimited"`.
    #[schemars(description = "The largest table, its heap plus its TOAST, the controller repacks, or `\"unlimited\"` to repack a table of any size.")]
    pub max_relation_size: Ceiling<HumanByteSize>,
}

impl Default for RepackConfig {
    fn default() -> RepackConfig {
        RepackConfig {
            mode: MaintenanceMode::Observe,
            max_relation_size: Ceiling::At(HumanByteSize::gib(4)),
        }
    }
}

/// Adaptive PostgreSQL maintenance configuration.
#[derive(Clone, Copy, Deserialize, Serialize, Default, JsonSchema, Debug, Eq, PartialEq)]
#[schemars(description = "Adaptive PostgreSQL inspection and maintenance of the tenant databases.")]
#[schemars(deny_unknown_fields)]
pub struct MaintenanceConfig {
    /// Whether inspection is disabled, observational, or automatic.
    #[schemars(
        description = "Whether maintenance is off, only inspects and reports, or also acts on what it finds. Acts by default, so a deployment nobody tunes still gets its wraparound and WAL retention warnings, its catch-up vacuums and its index rebuilds. An action whose right the broker's role lacks is only reported. Set `\"observe\"` where the database is maintained on its own schedule."
    )]
    pub mode: MaintenanceMode,
    /// Fixed threshold and cadence profile.
    #[schemars(description = "How often the controller inspects, and how much evidence it needs before it acts.")]
    pub profile: MaintenanceProfile,
    /// Online repacks of bloated tables.
    #[schemars(description = "Online repacks of bloated tables.")]
    pub repack: RepackConfig,
}

#[cfg(test)]
mod tests {
    use crate::{
        human_byte_size::HumanByteSize,
        schema::{
            ceiling::{Ceiling, Unlimited},
            database::maintenance::{MaintenanceConfig, MaintenanceMode, MaintenanceProfile, RepackConfig},
        },
    };
    use pretty_assertions::assert_eq;
    use toml::de::Error;

    #[test]
    fn omitted_maintenance_configuration_is_automatic_and_conservative() {
        let config = MaintenanceConfig::default();
        assert_eq!(config.mode, MaintenanceMode::Automatic);
        assert_eq!(config.profile, MaintenanceProfile::Conservative);
    }

    #[test]
    fn omitted_repack_configuration_only_reports_and_stops_at_four_gibibytes() {
        let config = MaintenanceConfig::default();
        assert_eq!(config.repack.mode, MaintenanceMode::Observe);
        assert_eq!(config.repack.max_relation_size, Ceiling::At(HumanByteSize::gib(4)));
        assert_eq!(config.repack.max_relation_size.limit().map(HumanByteSize::as_u64), Some(4 << 30));
    }

    #[test]
    fn a_repack_table_reads_every_mode_and_an_unlimited_relation_size() -> Result<(), Error> {
        for (raw, mode) in [
            ("disabled", MaintenanceMode::Disabled),
            ("observe", MaintenanceMode::Observe),
            ("automatic", MaintenanceMode::Automatic),
        ] {
            let config: RepackConfig = toml::from_str(&format!("mode = \"{raw}\"\nmax_relation_size = \"unlimited\"\n"))?;
            assert_eq!(
                config,
                RepackConfig {
                    mode,
                    max_relation_size: Ceiling::Unlimited(Unlimited::Unlimited),
                }
            );
        }
        Ok(())
    }

    #[test]
    fn a_repack_size_that_is_neither_a_size_nor_unlimited_is_refused() {
        assert!(toml::from_str::<RepackConfig>("mode = \"observe\"\nmax_relation_size = \"forever\"\n").is_err());
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
