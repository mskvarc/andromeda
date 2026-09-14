//! Active-component assembly from composition metadata and selected adapters.

#![expect(clippy::absolute_paths, reason = "shadow generates absolute standard-library paths")]

use andromeda_component_report::{
    BuildBranch,
    BuildCommit,
    BuildOperatingSystem,
    BuildTarget,
    BuildTimestamp,
    CargoVersion,
    ComponentReport,
    ComponentVersion,
    RustChannel,
    RustVersion,
};
use andromeda_postgresql_context_store::component_report;
use shadow_rs::shadow;
use sqlx::PgPool;
use thiserror::Error;

shadow!(build);

/// Failure while discovering active concrete broker components.
#[derive(Debug, Error)]
pub(crate) enum ComponentReportError {
    /// PostgreSQL component discovery failed.
    #[error("active PostgreSQL component discovery failed: {0}")]
    PostgreSql(#[from] sqlx::Error),
}

/// Collect build metadata and reports from the selected PostgreSQL adapter.
pub(crate) async fn collect(pool: &PgPool) -> Result<Vec<ComponentReport>, ComponentReportError> {
    let mut reports = component_report::discover(pool).await?;
    reports.insert(0, andromeda_report());
    Ok(reports)
}

/// Construct the report for the running Andromeda build.
fn andromeda_report() -> ComponentReport {
    ComponentReport::Andromeda {
        version: ComponentVersion::from(format!("v{}", build::PKG_VERSION)),
        build_time: BuildTimestamp::from(build::BUILD_TIME_3339.to_owned()),
        branch: BuildBranch::from(build::BRANCH.to_owned()),
        commit: BuildCommit::from(build::COMMIT_HASH.to_owned()),
        git_clean: build::GIT_CLEAN,
        rust_version: RustVersion::from(build::RUST_VERSION.to_owned()),
        rust_channel: RustChannel::from(build::RUST_CHANNEL.to_owned()),
        cargo_version: CargoVersion::from(build::CARGO_VERSION.to_owned()),
        build_operating_system: BuildOperatingSystem::from(build::BUILD_OS.to_owned()),
        build_target: BuildTarget::from(build::BUILD_TARGET.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use crate::component_report::{andromeda_report, build};
    use andromeda_component_report::ComponentReport;
    use pretty_assertions::assert_ne;

    #[test]
    fn andromeda_report_uses_build_constants() {
        assert!(matches!(andromeda_report(), ComponentReport::Andromeda { .. }));
        assert_ne!(build::BUILD_TIME_3339, "");
    }
}
