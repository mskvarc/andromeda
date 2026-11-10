//! The maintenance evidence a test seeds in the platform database of the broker it runs against, so a decision that
//! takes hours of evidence in production is due within the test.
//!
//! A repack qualifies on free space that lasted the profile's qualifying duration, six hours in the responsive
//! profile, over at least two samples. Seeding a target's breach run seven hours back leaves one more sample to take:
//! the broker's next free-space probe of the heap queues the repack if the heap still qualifies, and judges it again
//! before it runs, so nothing the broker would refuse is forced.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
};
use andromeda_postgresql_seaquery::{interval::make_interval_secs, statement::Statement};
use ngsi_ld::model::Tenant;
use sea_query::{Alias, Expr, ExprTrait, OnConflict, Query, SimpleExpr, error::Error as SeaQueryError};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

/// The longest a seed waits for its one connection.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// How far back a seeded breach run started: an hour past the responsive profile's repack qualifying duration.
const SEEDED_BREACH_SECONDS: i64 = 7 * 3_600;

/// The repack target of `current_attributes`, as the platform migration numbers it.
pub const CURRENT_ATTRIBUTES_REPACK: i16 = 46;

/// Why a seed could not be written.
#[derive(Debug, thiserror::Error)]
pub enum MaintenanceSeedError {
    /// The stack names no server for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The server could not be reached or the statement failed.
    #[error("the platform database could not be written")]
    Server(#[from] sqlx::Error),
    /// The statement could not be assembled.
    #[error("the seed statement could not be assembled")]
    Statement(#[from] SeaQueryError),
}

/// A direct connection to the platform database of one broker's server.
#[derive(Debug, Clone)]
pub struct MaintenanceSeed {
    /// One connection, opened on first use.
    pool: PgPool,
}

impl MaintenanceSeed {
    /// The platform database of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`MaintenanceSeedError::Config`] when the stack names no such server.
    pub fn of(role: BrokerRole) -> Result<MaintenanceSeed, MaintenanceSeedError> {
        let config = SuiteConfig::get().map_err(Clone::clone)?;
        let server = config.servers().server(role)?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_lazy(server.as_str())?;
        Ok(MaintenanceSeed { pool })
    }

    /// Opens a breach run of `target` in the live branch of `tenant`, seven hours back, with one sample, and clears
    /// its cooldown: the insert sets none, so the conflict takes the column's default.
    ///
    /// # Errors
    ///
    /// Returns [`MaintenanceSeedError`] when the platform database cannot be written.
    pub async fn seed_breach(&self, tenant: &Tenant, target: i16) -> Result<(), MaintenanceSeedError> {
        let column = |name: &str| Alias::new(name);
        let since: SimpleExpr = Expr::current_timestamp().sub(make_interval_secs(Expr::val(SEEDED_BREACH_SECONDS)));
        let tenant_key = Query::select()
            .column(column("tenant_key"))
            .expr(Expr::val(0_i64))
            .expr(Expr::val(target))
            .expr(since)
            .expr(Expr::val(1_i32))
            .from(column("tenants"))
            .and_where(Expr::col(column("name")).eq(AsRef::<str>::as_ref(tenant)))
            .to_owned();
        let statement = Query::insert()
            .into_table(column("postgresql_maintenance_targets"))
            .columns([
                column("tenant_key"),
                column("snapshot_key"),
                column("target"),
                column("first_breach_at"),
                column("breach_samples"),
            ])
            .select_from(tenant_key)?
            .on_conflict(
                OnConflict::columns([column("tenant_key"), column("snapshot_key"), column("target")])
                    .update_columns([column("first_breach_at"), column("breach_samples"), column("cooldown_until")])
                    .to_owned(),
            )
            .to_owned();
        statement.to_query().execute(&self.pool).await?;
        Ok(())
    }
}
