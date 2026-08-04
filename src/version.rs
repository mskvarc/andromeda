//! Version service that collects build-time metadata and runtime database
//! extension versions.

use crate::core::error::AndromedaError;
use andromeda_domain::version::{AndromedaVersion, ExtensionVersion, TimescaleDbVersion, VersionInfo};
use shadow_rs::shadow;
use sqlx::PgPool;

shadow!(build);

/// Version information collector backed by build-time constants and a database
/// connection for querying extension versions.
#[derive(Clone)]
pub struct Version {
    /// Database connection pool for querying extension versions.
    pool: PgPool,
}

impl Version {
    /// Create a new [`Version`] service with the given database pool.
    pub const fn new(pool: PgPool) -> Version {
        Version { pool }
    }

    /// Collect build-time and runtime version information.
    ///
    /// Queries PostgreSQL for server version and installed extension versions.
    /// Extensions that are not installed report `"not installed"` as their version.
    pub async fn check(&self) -> Result<VersionInfo, AndromedaError> {
        let andromeda = AndromedaVersion {
            version: format!("v{}", build::PKG_VERSION),
            build_time: build::BUILD_TIME_3339.to_owned(),
            branch: build::BRANCH.to_owned(),
            commit_hash: build::COMMIT_HASH.to_owned(),
            git_clean: build::GIT_CLEAN,
            rust_version: build::RUST_VERSION.to_owned(),
            rust_channel: build::RUST_CHANNEL.to_owned(),
            cargo_version: build::CARGO_VERSION.to_owned(),
            build_os: build::BUILD_OS.to_owned(),
            build_target: build::BUILD_TARGET.to_owned(),
        };

        let row = sqlx::query(
            "SELECT \
                current_setting('server_version') AS pg_version, \
                (SELECT extversion FROM pg_extension WHERE extname = 'postgis') AS postgis_version, \
                (SELECT extversion FROM pg_extension WHERE extname = 'timescaledb') AS timescaledb_version, \
                (SELECT current_setting('timescaledb.license')) AS timescaledb_license",
        )
        .fetch_one(&self.pool)
        .await?;

        let pg_version: String = sqlx::Row::get(&row, "pg_version");
        let postgis_version: Option<String> = sqlx::Row::get(&row, "postgis_version");
        let timescaledb_version: Option<String> = sqlx::Row::get(&row, "timescaledb_version");
        let timescaledb_license: Option<String> = sqlx::Row::get(&row, "timescaledb_license");

        let postgresql = ExtensionVersion {
            version: format!("v{pg_version}"),
        };
        let postgis = ExtensionVersion {
            version: postgis_version.map_or_else(|| "not installed".to_owned(), |v| format!("v{v}")),
        };

        let license = timescaledb_license.map_or_else(
            || "unknown".to_owned(),
            |l| match l.as_str() {
                "timescale" => "TSL".to_owned(),
                "apache" => "Apache-2.0".to_owned(),
                other => other.to_owned(),
            },
        );
        let timescaledb = TimescaleDbVersion {
            version: timescaledb_version.map_or_else(|| "not installed".to_owned(), |v| format!("v{v}")),
            license,
        };

        Ok(VersionInfo {
            andromeda,
            postgresql,
            postgis,
            timescaledb,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::build;

    #[test]
    fn andromeda_version_from_build_constants() {
        let version = format!("v{}", build::PKG_VERSION);
        assert!(version.starts_with("v"));
        assert!(!build::BUILD_TIME_3339.is_empty());
        // GIT_CLEAN is bool -- this compiles only if the type is correct.
        let _clean: bool = build::GIT_CLEAN;
    }
}
