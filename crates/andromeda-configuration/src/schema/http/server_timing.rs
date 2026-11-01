//! `Server-Timing` policy: whether it is emitted, and which stack phases appear.

use crate::{comma_list::array_or_comma_list, schema::feature_mode::FeatureMode};
use enumset::{EnumSet, EnumSetType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// One stack-level span the broker can report in the `Server-Timing` header.
///
/// These are the phases measurable at the HTTP-stack boundary. Deeper
/// per-subsystem spans (database query, JSON-LD expansion, rendering) are
/// recorded by their owning crates into the same response extension and are not
/// enumerated here.
#[derive(Debug, Display, EnumString, Deserialize, Serialize, EnumSetType, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[enumset(serialize_repr = "list")]
#[schemars(description = "One time the `Server-Timing` header can report.")]
pub enum ServerTimingPhase {
    /// The whole request as seen at the stack boundary. Emitted by the base
    /// timing layer whenever `Server-Timing` is enabled.
    #[schemars(description = "Reports the whole request, as seen where it enters the HTTP stack. Present whenever `Server-Timing` is on.")]
    Total,
    /// The span from the stack entry point to handler dispatch and back.
    #[schemars(description = "Reports the time from the HTTP stack's entry to the handler and back.")]
    Dispatch,
}

/// `Server-Timing` configuration.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "Whether responses carry a `Server-Timing` header, and which times it reports.")]
pub struct ServerTimingConfig {
    /// Whether the broker emits a `Server-Timing` response header at all.
    #[schemars(description = "Whether responses carry a `Server-Timing` header.")]
    pub mode: FeatureMode,
    /// Which stack-level phases are reported when the header is emitted.
    #[serde(deserialize_with = "array_or_comma_list")]
    #[schemars(with = "Vec<ServerTimingPhase>", description = "Which times the header reports.")]
    pub phases: EnumSet<ServerTimingPhase>,
}

impl Default for ServerTimingConfig {
    fn default() -> ServerTimingConfig {
        ServerTimingConfig {
            mode: FeatureMode::Enabled,
            phases: EnumSet::only(ServerTimingPhase::Total),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{
        feature_mode::FeatureMode,
        http::server_timing::{ServerTimingConfig, ServerTimingPhase},
    };
    use enumset::EnumSet;

    #[test]
    fn enabled_by_default_with_the_total_phase() {
        let config = ServerTimingConfig::default();
        pretty_assertions::assert_eq!(config.mode, FeatureMode::Enabled);
        pretty_assertions::assert_eq!(config.phases, EnumSet::only(ServerTimingPhase::Total));
    }

    #[test]
    fn phases_parse_from_toml() {
        let config: ServerTimingConfig = toml::from_str("mode = \"enabled\"\nphases = [\"total\", \"dispatch\"]").expect("server timing parses");
        pretty_assertions::assert_eq!(config.phases, ServerTimingPhase::Total | ServerTimingPhase::Dispatch);
    }

    #[test]
    fn server_timing_config_round_trips_through_toml() {
        let config = ServerTimingConfig {
            mode: FeatureMode::Disabled,
            phases: ServerTimingPhase::Dispatch.into(),
        };
        let rendered = toml::to_string(&config).expect("server timing serialises");
        let parsed: ServerTimingConfig = toml::from_str(&rendered).expect("server timing parses");
        pretty_assertions::assert_eq!(parsed, config);
    }
}
