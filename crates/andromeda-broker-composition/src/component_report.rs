//! Active-component assembly from composition metadata and selected adapters.

#![expect(clippy::absolute_paths, reason = "shadow generates absolute standard-library paths")]

use crate::broker::MAINTENANCE_PLATFORM_POOL_CONNECTIONS;
use andromeda_component_report::{
    BuildBranch,
    BuildCommit,
    BuildOperatingSystem,
    BuildTarget,
    BuildTimestamp,
    CapacityVerdict,
    CargoVersion,
    ComponentReport,
    ComponentVersion,
    RustChannel,
    RustVersion,
};
use andromeda_configuration::broker_config::BrokerConfig;
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

/// The connection the maintenance controller opens into the tenant it visits.
const MAINTENANCE_TENANT_CONNECTIONS: u32 = 1;

/// The one-connection pools the dormant-tenant probes open on the request
/// path: the pending-work probe and the usage probe.
const REQUEST_PROBE_CONNECTIONS: u32 = 2;

/// The connections one cold tenant build holds at its peak: the extension
/// update's lock holder beside the connection running an update; the
/// migrator's connection opens after both close.
const BUILD_CONNECTIONS: u32 = 2;

/// Collect build metadata and reports from the selected PostgreSQL adapter,
/// warning when the broker's pools could need more connections than the
/// server lets it open.
pub(crate) async fn collect(pool: &PgPool, config: &BrokerConfig) -> Result<Vec<ComponentReport>, ComponentReportError> {
    let ceiling = broker_connection_ceiling(config);
    let mut reports = component_report::discover(pool, ceiling).await?;
    for report in &reports {
        if let ComponentReport::PostgreSql { connections, .. } = report
            && connections.verdict == CapacityVerdict::Exceeded
        {
            tracing::warn!(
                server_max_connections = connections.server_max_connections,
                reserved_for_others = connections.reserved_for_others,
                broker_ceiling = connections.broker_ceiling,
                event = "database_connection_capacity_exceeded",
                "the broker's pools, all full at once, need more connections than the server lets its role open"
            );
        }
    }
    reports.insert(0, andromeda_report());
    Ok(reports)
}

/// The connections the broker holds against PostgreSQL when every pool it
/// may open is full at once.
///
/// Straight to PostgreSQL, always: the platform pool and the maintenance
/// controller's platform pool, its connection into the tenant it visits, and
/// the peak of every cold build that may run at once. On the request path,
/// unless a transaction pooler fronts it: the default tenant's serving pool,
/// one serving pool per tenant the runtime cache holds, and the
/// dormant-tenant probes. Behind a pooler those reach the pooler, whose own
/// server connections are the pooler's to size.
fn broker_connection_ceiling(config: &BrokerConfig) -> u32 {
    let database = &config.database;
    let tenancy = &config.tenancy;
    let builds = u32::try_from(tenancy.build.max_concurrent.get()).unwrap_or(u32::MAX);
    let direct = database
        .max_connections
        .saturating_add(MAINTENANCE_PLATFORM_POOL_CONNECTIONS)
        .saturating_add(MAINTENANCE_TENANT_CONNECTIONS)
        .saturating_add(builds.saturating_mul(BUILD_CONNECTIONS));
    if database.is_pooled() {
        return direct;
    }
    let cached = u32::try_from(tenancy.residency.max_resident.get()).unwrap_or(u32::MAX);
    direct
        .saturating_add(database.max_connections)
        .saturating_add(cached.saturating_mul(tenancy.databases.max_connections.get()))
        .saturating_add(REQUEST_PROBE_CONNECTIONS)
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
    use crate::component_report::{andromeda_report, broker_connection_ceiling, build};
    use andromeda_component_report::ComponentReport;
    use andromeda_configuration::{broker_config::BrokerConfig, schema::database::pooler::PoolerConfig};
    use pretty_assertions::{assert_eq, assert_ne};
    use std::num::{NonZeroU32, NonZeroUsize};

    /// A configuration with small, distinct pool sizes.
    fn sized() -> BrokerConfig {
        let mut config = BrokerConfig::default();
        config.database.max_connections = 10;
        config.tenancy.build.max_concurrent = NonZeroUsize::MIN.saturating_add(3);
        config.tenancy.residency.max_resident = NonZeroUsize::MIN.saturating_add(19);
        config.tenancy.databases.max_connections = NonZeroU32::MIN.saturating_add(2);
        config
    }

    /// Direct: platform 10, maintenance 2 plus its tenant connection 1,
    /// builds 4 x 2; request path: default tenant 10, cached tenants 20 x 3,
    /// probes 2.
    #[test]
    fn a_direct_broker_counts_every_pool_it_opens_against_postgresql() {
        assert_eq!(broker_connection_ceiling(&sized()), 10 + 2 + 1 + 8 + 10 + 60 + 2);
    }

    /// Behind a pooler the request path reaches the pooler, so only the
    /// direct pools count.
    #[test]
    fn behind_a_pooler_only_the_direct_pools_count() {
        let mut config = sized();
        config.database.pooler = Some(PoolerConfig::default());
        assert_eq!(broker_connection_ceiling(&config), 10 + 2 + 1 + 8);
    }

    #[test]
    fn andromeda_report_uses_build_constants() {
        assert!(matches!(andromeda_report(), ComponentReport::Andromeda { .. }));
        assert_ne!(build::BUILD_TIME_3339, "");
    }
}
