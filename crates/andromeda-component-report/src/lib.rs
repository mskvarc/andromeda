//! Typed reports for concrete components active in one broker profile.

#![deny(missing_docs)]

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
    /// RFC 3339 build timestamp emitted by the build metadata generator.
    BuildTimestamp
);
report_text!(
    /// Source-control branch recorded at build time.
    BuildBranch
);
report_text!(
    /// Source-control commit identifier recorded at build time.
    BuildCommit
);
report_text!(
    /// Rust compiler version recorded at build time.
    RustVersion
);
report_text!(
    /// Rust release channel recorded at build time.
    RustChannel
);
report_text!(
    /// Cargo version recorded at build time.
    CargoVersion
);
report_text!(
    /// Build host operating-system description.
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

/// One active runtime or build component.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "component")]
pub enum ComponentReport {
    /// Andromeda build metadata.
    #[serde(rename = "andromeda", rename_all = "camelCase")]
    Andromeda {
        /// Andromeda package version.
        version: ComponentVersion,
        /// Build timestamp.
        build_time: BuildTimestamp,
        /// Source-control branch.
        branch: BuildBranch,
        /// Source-control commit.
        commit: BuildCommit,
        /// Whether the source tree was clean when built.
        git_clean: bool,
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
    use crate::{ComponentReport, ComponentVersion};

    #[test]
    fn component_variants_use_stable_product_names() -> Result<(), serde_json::Error> {
        let value = serde_json::to_value(ComponentReport::PostgreSql {
            version: ComponentVersion::from("16.2".to_owned()),
        })?;
        pretty_assertions::assert_eq!(value["component"], "postgresql");
        pretty_assertions::assert_eq!(value["version"], "16.2");
        Ok(())
    }
}

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}
