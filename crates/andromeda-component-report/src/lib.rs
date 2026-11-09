//! Typed reports for concrete components active in one broker profile.

use derive_more::{Display, From};
use serde::{Deserialize, Serialize};

/// Define one opaque build or component metadata value with a distinct semantic owner.
macro_rules! report_text {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Deserialize, Display, From, Eq, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
    };
}

report_text!(
    /// Version text reported by one active component.
    ComponentVersion
);
report_text!(
    /// The compiler's version line, as `rustc --version` prints it.
    RustVersion
);
report_text!(
    /// The compiler's release channel: `stable`, `beta`, `nightly` or `dev`.
    RustChannel
);
report_text!(
    /// Cargo's version line, as `cargo --version` prints it.
    CargoVersion
);
report_text!(
    /// Operating system and architecture of the machine that compiled the
    /// broker, as `os-arch`.
    BuildOperatingSystem
);
report_text!(
    /// Rust target triple used for the build.
    BuildTarget
);
report_text!(
    /// License identifier reported by `TimescaleDB`.
    TimescaleLicense
);

/// Whether the server holds enough connections for every pool the broker may
/// open at once.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CapacityVerdict {
    /// The broker's pools fit in the connections it may use.
    Sufficient,
    /// The broker's pools, all full at once, need more connections than it
    /// may use; the server refuses the connections past that.
    Exceeded,
}

/// The broker's connection ceiling against what the server lets it use.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCapacity {
    /// The server's `max_connections`.
    pub server_max_connections: u32,
    /// The slots the broker's role may not take: `superuser_reserved_connections`
    /// unless it is a superuser, and `reserved_connections` unless it is a
    /// superuser or a member of `pg_use_reserved_connections`.
    pub reserved_for_others: u32,
    /// The connections the broker's pools hold when every one of them is
    /// full at once.
    pub broker_ceiling: u32,
    /// Whether the ceiling fits in the slots the broker may use.
    pub verdict: CapacityVerdict,
}

impl ConnectionCapacity {
    /// The capacity of a server with `server_max_connections`, of which
    /// `reserved_for_others` are not the broker's, for a broker whose pools
    /// may hold `broker_ceiling` connections.
    #[must_use]
    pub const fn new(server_max_connections: u32, reserved_for_others: u32, broker_ceiling: u32) -> ConnectionCapacity {
        let verdict = if broker_ceiling <= server_max_connections.saturating_sub(reserved_for_others) {
            CapacityVerdict::Sufficient
        } else {
            CapacityVerdict::Exceeded
        };
        ConnectionCapacity {
            server_max_connections,
            reserved_for_others,
            broker_ceiling,
            verdict,
        }
    }
}

/// One active runtime or build component.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "component", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ComponentReport {
    /// The broker's own build: its release, and the toolchain and target it
    /// was compiled with. It names nothing of the repository or of when the
    /// build ran, so the same sources report the same build.
    Andromeda {
        /// Andromeda package version.
        version: ComponentVersion,
        /// Rust compiler version.
        rust_version: RustVersion,
        /// Rust release channel.
        rust_channel: RustChannel,
        /// Cargo version.
        cargo_version: CargoVersion,
        /// Build host operating-system description.
        build_operating_system: BuildOperatingSystem,
        /// Rust target triple.
        build_target: BuildTarget,
    },
    /// Active PostgreSQL server.
    #[serde(rename = "postgresql")]
    PostgreSql {
        /// Server version.
        version: ComponentVersion,
        /// The broker's connection ceiling against the server's limit.
        connections: ConnectionCapacity,
    },
    /// Active `PostGIS` extension.
    #[serde(rename = "postgis")]
    PostGis {
        /// Extension version.
        version: ComponentVersion,
    },
    /// Active `TimescaleDB` extension.
    #[serde(rename = "timescaledb")]
    TimescaleDb {
        /// Extension version.
        version: ComponentVersion,
        /// Active extension license.
        license: TimescaleLicense,
    },
}

#[cfg(test)]
mod tests {
    use crate::{
        BuildOperatingSystem,
        BuildTarget,
        CapacityVerdict,
        CargoVersion,
        ComponentReport,
        ComponentVersion,
        ConnectionCapacity,
        RustChannel,
        RustVersion,
    };

    #[test]
    fn the_andromeda_component_carries_its_release_and_toolchain_and_nothing_else() -> Result<(), serde_json::Error> {
        let value = serde_json::to_value(ComponentReport::Andromeda {
            version: ComponentVersion::from("v1.0.0".to_owned()),
            rust_version: RustVersion::from("rustc 1.99.0-nightly (0123456789 2026-08-20)".to_owned()),
            rust_channel: RustChannel::from("nightly".to_owned()),
            cargo_version: CargoVersion::from("cargo 1.99.0-nightly (abcdef0123 2026-08-19)".to_owned()),
            build_operating_system: BuildOperatingSystem::from("linux-aarch64".to_owned()),
            build_target: BuildTarget::from("aarch64-unknown-linux-gnu".to_owned()),
        })?;
        pretty_assertions::assert_eq!(
            value,
            serde_json::json!({
                "component": "andromeda",
                "version": "v1.0.0",
                "rustVersion": "rustc 1.99.0-nightly (0123456789 2026-08-20)",
                "rustChannel": "nightly",
                "cargoVersion": "cargo 1.99.0-nightly (abcdef0123 2026-08-19)",
                "buildOperatingSystem": "linux-aarch64",
                "buildTarget": "aarch64-unknown-linux-gnu",
            })
        );
        Ok(())
    }

    #[test]
    fn component_variants_use_stable_product_names() -> Result<(), serde_json::Error> {
        let value = serde_json::to_value(ComponentReport::PostgreSql {
            version: ComponentVersion::from("16.2".to_owned()),
            connections: ConnectionCapacity::new(200, 3, 150),
        })?;
        pretty_assertions::assert_eq!(value["component"], "postgresql");
        pretty_assertions::assert_eq!(value["version"], "16.2");
        pretty_assertions::assert_eq!(value["connections"]["brokerCeiling"], 150);
        pretty_assertions::assert_eq!(value["connections"]["verdict"], "sufficient");
        Ok(())
    }

    #[test]
    fn the_ceiling_must_fit_in_the_slots_left_after_the_reserved_ones() {
        pretty_assertions::assert_eq!(ConnectionCapacity::new(100, 6, 94).verdict, CapacityVerdict::Sufficient);
        pretty_assertions::assert_eq!(ConnectionCapacity::new(100, 6, 95).verdict, CapacityVerdict::Exceeded);
        pretty_assertions::assert_eq!(ConnectionCapacity::new(4, 6, 1).verdict, CapacityVerdict::Exceeded);
    }
}

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}
