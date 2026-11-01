//! A runtime setting as a session the pooler in front of a broker's server opens for one database sees it.
//!
//! A database default reaches only the sessions that start after it is written, and a transaction pooler keeps its
//! server sessions and hands them to later clients. Reading through the pooler therefore shows the value a pooled
//! request of the broker runs under, which a direct connection opened afresh would not.

use crate::{database_name::DatabaseName, pooler_console::PoolerConsole};
use andromeda_postgresql_seaquery::{cast::cast, introspection::current_setting, pg_type::PostgresType, setting::Setting, statement::Statement};
use sea_query::{Query, SelectStatement};
use sqlx::{Connection, PgConnection};

/// Why a setting could not be read through the pooler.
#[derive(Debug, thiserror::Error)]
pub enum PooledSettingError {
    /// The pooler could not be reached, the database could not be routed, or the statement failed.
    #[error("the setting {setting} could not be read through the pooler")]
    Pooler {
        /// The setting read.
        setting: Setting,
        /// Why the read failed.
        #[source]
        source: sqlx::Error,
    },
}

/// Whether the boolean `setting` is on in a session the pooler of `console` serves for `database`.
///
/// # Errors
///
/// Returns [`PooledSettingError::Pooler`] when the pooler or the statement fails, or the setting holds no boolean.
pub async fn pooled_flag(console: &PoolerConsole, database: &DatabaseName, setting: Setting) -> Result<bool, PooledSettingError> {
    let failed = |source| PooledSettingError::Pooler { setting, source };
    let mut connection = PgConnection::connect(console.database_url(database).as_str()).await.map_err(failed)?;
    let flag = flag_statement(setting).to_query_scalar().fetch_one(&mut connection).await;
    connection.close().await.map_err(failed)?;
    flag.map_err(failed)
}

/// `SELECT current_setting(<setting>)::boolean`.
fn flag_statement(setting: Setting) -> SelectStatement {
    Query::select().expr(cast(current_setting(setting), PostgresType::Boolean)).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::pooled_setting::flag_statement;
    use andromeda_postgresql_seaquery::setting::Setting;
    use googletest::prelude::*;
    use sea_query::PostgresQueryBuilder;

    #[gtest]
    fn a_flag_is_read_as_its_setting_cast_to_boolean() {
        let (sql, _values) = flag_statement(Setting::TimescaledbEnableChunkSkipping).build(PostgresQueryBuilder);
        expect_that!(sql, eq("SELECT current_setting($1)::boolean"));
    }
}
