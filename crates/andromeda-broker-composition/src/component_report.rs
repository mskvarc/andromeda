//! Active-component assembly from composition metadata and selected adapters.

use crate::{broker::MAINTENANCE_PLATFORM_POOL_CONNECTIONS, build_metadata};
use andromeda_component_report::{
    BuildOperatingSystem,
    BuildTarget,
    CapacityVerdict,
    CargoVersion,
    ComponentReport,
    ComponentVersion,
    RustChannel,
    RustVersion,
};
use andromeda_configuration::{
    broker_config::BrokerConfig,
    schema::{feature_mode::FeatureMode, tenancy::TenancyConfig},
};
use andromeda_postgresql_context_store::component_report;
use andromeda_postgresql_maintenance::repack_task::RUN_SESSIONS;
use sqlx::PgPool;
use thiserror::Error;

/// Failure while discovering active concrete broker components.
#[derive(Debug, Error)]
pub(crate) enum ComponentReportError {
    /// PostgreSQL component discovery failed.
    #[error("active PostgreSQL component discovery failed: {0}")]
    PostgreSql(#[from] sqlx::Error),
}

/// The connection the maintenance controller's pass opens into the tenant it
/// visits.
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
/// controller's platform pool, its pass's connection into the tenant it
/// visits, the repack task's sessions into the tenant whose table it
/// repacks, and the peak of every cold build that may run at once. On the request path,
/// unless a transaction pooler fronts it: the default tenant's serving pool,
/// one serving pool per tenant the runtime cache holds while named tenancy is
/// on, and the dormant-tenant probes, which the broker builds whether or not
/// named tenancy is on. Behind a pooler those reach the pooler, whose own
/// server connections are the pooler's to size.
fn broker_connection_ceiling(config: &BrokerConfig) -> u32 {
    let database = &config.database;
    let tenancy = &config.tenancy;
    let builds = u32::try_from(tenancy.build.max_concurrent.get()).unwrap_or(u32::MAX);
    let direct = database
        .platform_max_connections
        .get()
        .saturating_add(MAINTENANCE_PLATFORM_POOL_CONNECTIONS)
        .saturating_add(MAINTENANCE_TENANT_CONNECTIONS)
        .saturating_add(RUN_SESSIONS)
        .saturating_add(builds.saturating_mul(BUILD_CONNECTIONS));
    if database.is_pooled() {
        return direct;
    }
    direct
        .saturating_add(tenancy.default_tenant.max_connections.get())
        .saturating_add(named_tenant_connections(tenancy))
        .saturating_add(REQUEST_PROBE_CONNECTIONS)
}

/// The serving pools of the named tenants the runtime cache holds at once;
/// none while named tenancy is off, since only the default tenant exists.
fn named_tenant_connections(tenancy: &TenancyConfig) -> u32 {
    match tenancy.mode {
        FeatureMode::Disabled => 0,
        FeatureMode::Enabled => u32::try_from(tenancy.residency.max_resident.get())
            .unwrap_or(u32::MAX)
            .saturating_mul(tenancy.databases.max_connections.get()),
    }
}

/// Construct the report for the running Andromeda build.
fn andromeda_report() -> ComponentReport {
    ComponentReport::Andromeda {
        version: ComponentVersion::from(format!("v{}", build_metadata::VERSION)),
        rust_version: RustVersion::from(build_metadata::RUST_VERSION.to_owned()),
        rust_channel: RustChannel::from(build_metadata::RUST_CHANNEL.to_owned()),
        cargo_version: CargoVersion::from(build_metadata::CARGO_VERSION.to_owned()),
        build_operating_system: BuildOperatingSystem::from(build_metadata::BUILD_OPERATING_SYSTEM.to_owned()),
        build_target: BuildTarget::from(build_metadata::BUILD_TARGET.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use crate::component_report::{andromeda_report, broker_connection_ceiling};
    use andromeda_component_report::ComponentReport;
    use andromeda_configuration::{
        broker_config::BrokerConfig,
        schema::{
            database::{connection::PostgresConnection, pooler::PoolerConfig},
            feature_mode::FeatureMode,
        },
    };
    use pretty_assertions::assert_eq;
    use std::num::{NonZeroU32, NonZeroUsize};
    use url_macro::url;

    /// A configuration with small, distinct pool sizes.
    fn sized() -> BrokerConfig {
        let mut config = BrokerConfig::new(PostgresConnection::new(url!("postgres://andromeda@localhost:5432/andromeda")));
        config.database.platform_max_connections = NonZeroU32::MIN.saturating_add(9);
        config.tenancy.default_tenant.max_connections = NonZeroU32::MIN.saturating_add(6);
        config.tenancy.build.max_concurrent = NonZeroUsize::MIN.saturating_add(3);
        config.tenancy.residency.max_resident = NonZeroUsize::MIN.saturating_add(19);
        config.tenancy.databases.max_connections = NonZeroU32::MIN.saturating_add(2);
        config
    }

    /// Direct: platform 10, maintenance 3 plus its pass's tenant connection
    /// 1 and its repack's 2, builds 4 x 2; request path: default tenant 7,
    /// cached tenants 20 x 3, probes 2.
    #[test]
    fn a_direct_broker_counts_every_pool_it_opens_against_postgresql() {
        let mut config = sized();
        config.tenancy.mode = FeatureMode::Enabled;
        assert_eq!(broker_connection_ceiling(&config), 10 + 3 + 1 + 2 + 8 + 7 + 60 + 2);
    }

    /// With named tenancy off only the default tenant's pool serves requests,
    /// so the 20 x 3 cached tenants drop out and the probes stay.
    #[test]
    fn with_named_tenancy_disabled_no_named_tenant_pool_counts() {
        let mut config = sized();
        config.tenancy.mode = FeatureMode::Disabled;
        assert_eq!(broker_connection_ceiling(&config), 10 + 3 + 1 + 2 + 8 + 7 + 2);
    }

    /// Behind a pooler the request path reaches the pooler, so only the
    /// direct pools count.
    #[test]
    fn behind_a_pooler_only_the_direct_pools_count() {
        let mut config = sized();
        config.database.pooler = Some(PoolerConfig {
            connection: PostgresConnection::new(url!("postgres://andromeda@localhost:6432/andromeda")),
            max_connections_per_tenant: NonZeroU32::MIN.saturating_add(1),
        });
        assert_eq!(broker_connection_ceiling(&config), 10 + 3 + 1 + 2 + 8);
    }

    #[test]
    fn the_andromeda_report_names_the_release_and_the_toolchain() {
        let report = andromeda_report();
        let ComponentReport::Andromeda {
            version,
            rust_version,
            rust_channel,
            cargo_version,
            build_operating_system,
            build_target,
        } = &report
        else {
            panic!("the broker's own report is not the andromeda component: {report:?}");
        };
        assert_eq!(version.to_string(), concat!("v", env!("CARGO_PKG_VERSION")));
        assert!(rust_version.to_string().starts_with("rustc "), "{rust_version}");
        assert!(
            ["stable", "beta", "nightly", "dev"].contains(&rust_channel.to_string().as_str()),
            "{rust_channel}"
        );
        assert!(cargo_version.to_string().starts_with("cargo "), "{cargo_version}");
        assert!(build_operating_system.to_string().contains('-'), "{build_operating_system}");
        assert!(build_target.to_string().contains('-'), "{build_target}");
    }
}
