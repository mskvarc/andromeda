//! The runtime configuration settings the broker reads and changes.
//!
//! A setting changed to a value computed at run time (the search path a
//! Snapshot branch migration runs under) goes through `set_config(name, value,
//! is_local)`, both bound. A setting that must be in force before the next
//! statement is analysed (the `TimescaleDB` loader reads its setting while it
//! analyses any statement, a `SELECT set_config(...)` included) is a `SET`
//! command, [`SettingAssignment`], its value a quoted literal.

use crate::{
    command::{Command, rendering::RenderedCommand},
    literal::quoted_text,
};
use sea_query::{Expr, Func, Iden, SimpleExpr};
use strum::{Display, IntoStaticStr};

/// A runtime configuration setting, rendered as PostgreSQL names it.
///
/// A per-table storage parameter that shares its name with a setting (the
/// autovacuum thresholds) is named by the same variant.
#[derive(Clone, Copy, Debug, Display, Eq, IntoStaticStr, PartialEq)]
pub enum Setting {
    /// `application_name`.
    #[strum(serialize = "application_name")]
    ApplicationName,
    /// `autovacuum_analyze_scale_factor`.
    #[strum(serialize = "autovacuum_analyze_scale_factor")]
    AutovacuumAnalyzeScaleFactor,
    /// `autovacuum_analyze_threshold`.
    #[strum(serialize = "autovacuum_analyze_threshold")]
    AutovacuumAnalyzeThreshold,
    /// `autovacuum_freeze_max_age`.
    #[strum(serialize = "autovacuum_freeze_max_age")]
    AutovacuumFreezeMaxAge,
    /// `autovacuum_max_workers`.
    #[strum(serialize = "autovacuum_max_workers")]
    AutovacuumMaxWorkers,
    /// `autovacuum_multixact_freeze_max_age`.
    #[strum(serialize = "autovacuum_multixact_freeze_max_age")]
    AutovacuumMultixactFreezeMaxAge,
    /// `autovacuum_vacuum_max_threshold`, which servers before PostgreSQL 18
    /// do not define.
    #[strum(serialize = "autovacuum_vacuum_max_threshold")]
    AutovacuumVacuumMaxThreshold,
    /// `autovacuum_vacuum_scale_factor`.
    #[strum(serialize = "autovacuum_vacuum_scale_factor")]
    AutovacuumVacuumScaleFactor,
    /// `autovacuum_vacuum_threshold`.
    #[strum(serialize = "autovacuum_vacuum_threshold")]
    AutovacuumVacuumThreshold,
    /// `block_size`.
    #[strum(serialize = "block_size")]
    BlockSize,
    /// `client_connection_check_interval`.
    #[strum(serialize = "client_connection_check_interval")]
    ClientConnectionCheckInterval,
    /// `deadlock_timeout`.
    #[strum(serialize = "deadlock_timeout")]
    DeadlockTimeout,
    /// `enable_seqscan`.
    #[strum(serialize = "enable_seqscan")]
    EnableSeqscan,
    /// `icu_validation_level`.
    #[strum(serialize = "icu_validation_level")]
    IcuValidationLevel,
    /// `lock_timeout`.
    #[strum(serialize = "lock_timeout")]
    LockTimeout,
    /// `max_connections`.
    #[strum(serialize = "max_connections")]
    MaxConnections,
    /// `max_parallel_maintenance_workers`.
    #[strum(serialize = "max_parallel_maintenance_workers")]
    MaxParallelMaintenanceWorkers,
    /// `max_parallel_workers_per_gather`.
    #[strum(serialize = "max_parallel_workers_per_gather")]
    MaxParallelWorkersPerGather,
    /// `pg_stat_statements.max`.
    #[strum(serialize = "pg_stat_statements.max")]
    PgStatStatementsMax,
    /// `plan_cache_mode`.
    #[strum(serialize = "plan_cache_mode")]
    PlanCacheMode,
    /// `reserved_connections`, which servers before PostgreSQL 16 do not
    /// define.
    #[strum(serialize = "reserved_connections")]
    ReservedConnections,
    /// `search_path`.
    #[strum(serialize = "search_path")]
    SearchPath,
    /// `server_encoding`.
    #[strum(serialize = "server_encoding")]
    ServerEncoding,
    /// `server_version`.
    #[strum(serialize = "server_version")]
    ServerVersion,
    /// `server_version_num`.
    #[strum(serialize = "server_version_num")]
    ServerVersionNum,
    /// `statement_timeout`.
    #[strum(serialize = "statement_timeout")]
    StatementTimeout,
    /// `superuser_reserved_connections`.
    #[strum(serialize = "superuser_reserved_connections")]
    SuperuserReservedConnections,
    /// `synchronous_commit`.
    #[strum(serialize = "synchronous_commit")]
    SynchronousCommit,
    /// `timescaledb.compress_truncate_behaviour`.
    #[strum(serialize = "timescaledb.compress_truncate_behaviour")]
    TimescaledbCompressTruncateBehaviour,
    /// `timescaledb.disable_load`.
    #[strum(serialize = "timescaledb.disable_load")]
    TimescaledbDisableLoad,
    /// `timescaledb.enable_chunk_skipping`.
    #[strum(serialize = "timescaledb.enable_chunk_skipping")]
    TimescaledbEnableChunkSkipping,
    /// `timescaledb.license`, which a server without `TimescaleDB` does not
    /// define.
    #[strum(serialize = "timescaledb.license")]
    TimescaledbLicense,
    /// `timescaledb.max_tuples_decompressed_per_dml_transaction`.
    #[strum(serialize = "timescaledb.max_tuples_decompressed_per_dml_transaction")]
    TimescaledbMaxTuplesDecompressedPerDmlTransaction,
}

impl Setting {
    /// The setting's name, as a bound text value.
    #[must_use]
    pub fn name_value(self) -> SimpleExpr {
        let name: &'static str = self.into();
        Expr::val(name)
    }
}

/// How long a changed setting lasts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingScope {
    /// Until the current transaction ends, as `SET LOCAL` does.
    Transaction,
    /// For the rest of the session, as a plain `SET` does.
    Session,
}

/// The `set_config` system function.
#[derive(Iden, Clone, Copy)]
#[iden = "set_config"]
struct SetConfig;

/// `set_config(name, value, is_local)`: change `setting` for `scope`.
#[must_use]
pub fn set_config(setting: Setting, value: &str, scope: SettingScope) -> SimpleExpr {
    let is_local = match scope {
        SettingScope::Transaction => true,
        SettingScope::Session => false,
    };
    Func::cust(SetConfig)
        .args([
            setting.name_value(),
            SimpleExpr::from(Expr::val(value.to_owned())),
            SimpleExpr::from(Expr::val(is_local)),
        ])
        .into()
}

/// `set_config('statement_timeout', '0', true)`: no statement of the current
/// transaction is cut short by the database's `statement_timeout`.
///
/// The broker installs a `statement_timeout` on each database just past the
/// request deadline, so a statement no request waits for any more stops. Work
/// no request bounds (copying a Snapshot, converting or dropping history
/// chunks, reading every location when a tenant opens) lifts it for its own
/// transaction.
#[must_use]
pub fn lift_statement_timeout() -> SimpleExpr {
    set_config(Setting::StatementTimeout, "0", SettingScope::Transaction)
}

/// `SET [LOCAL] <setting> = '<value>'`: change one setting for a session or a
/// transaction before the next statement is analysed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingAssignment {
    /// The setting changed.
    setting: Setting,
    /// The value, rendered as a quoted literal.
    value: String,
    /// How long the change lasts.
    scope: SettingScope,
}

impl SettingAssignment {
    /// Change `setting` to `value` for `scope`.
    pub fn new(setting: Setting, value: impl Into<String>, scope: SettingScope) -> SettingAssignment {
        SettingAssignment {
            setting,
            value: value.into(),
            scope,
        }
    }
}

impl RenderedCommand for SettingAssignment {
    fn rendered(&self) -> String {
        let local = match self.scope {
            SettingScope::Transaction => " LOCAL",
            SettingScope::Session => "",
        };
        format!("SET{local} {} = {}", self.setting, quoted_text(&self.value))
    }
}

impl Command for SettingAssignment {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        setting::{Setting, SettingAssignment, SettingScope, lift_statement_timeout, set_config},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn a_transaction_scoped_setting_is_local_and_binds_its_name_and_value() {
        let (sql, values) = Query::select()
            .expr(set_config(Setting::SearchPath, "snap_7, public", SettingScope::Transaction))
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT set_config($1, $2, $3)");
        assert_eq!(values.0.len(), 3);
        assert!(format!("{:?}", values.0[2]).contains("true"), "{values:?}");
    }

    #[test]
    fn a_session_scoped_setting_is_not_local() {
        let (_, values) = Query::select()
            .expr(set_config(Setting::SearchPath, "snap_7, public", SettingScope::Session))
            .build(PostgresQueryBuilder);
        assert!(format!("{:?}", values.0[2]).contains("false"), "{values:?}");
    }

    #[test]
    fn lifting_the_statement_timeout_lasts_until_the_transaction_ends() {
        assert_eq!(
            Query::select().expr(lift_statement_timeout()).to_string(PostgresQueryBuilder),
            "SELECT set_config('statement_timeout', '0', TRUE)"
        );
    }

    #[test]
    fn a_setting_command_quotes_its_value_and_scopes_itself() {
        assert_eq!(
            SettingAssignment::new(Setting::TimescaledbDisableLoad, "on", SettingScope::Transaction).rendered(),
            "SET LOCAL timescaledb.disable_load = 'on'"
        );
        assert_eq!(
            SettingAssignment::new(Setting::ApplicationName, "andromeda-postgresql-maintenance", SettingScope::Session).rendered(),
            "SET application_name = 'andromeda-postgresql-maintenance'"
        );
    }
}
