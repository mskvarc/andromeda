//! Reading and setting the search path of a connection.
//!
//! A Snapshot branch's statements name its relations unqualified, so a branch
//! schema is built and migrated with the schema first on the search path. The
//! setting lasts for a transaction, or for the session when the statements run
//! in transactions of their own.

use andromeda_postgresql_seaquery::{
    introspection::current_setting,
    search_path::SearchPath,
    setting::{Setting, SettingScope, set_config},
    statement::Statement,
};
use sea_query::{Query, SelectStatement};
use sqlx::PgConnection;

/// The connection's search path.
pub(crate) async fn current_search_path(conn: &mut PgConnection) -> Result<SearchPath, sqlx::Error> {
    let reported = current_search_path_statement().to_query_scalar::<String>().fetch_one(conn).await?;
    Ok(SearchPath::reported(reported))
}

/// Set the connection's search path to `search_path` for `scope`.
pub(crate) async fn steer_search_path(conn: &mut PgConnection, search_path: &SearchPath, scope: SettingScope) -> Result<(), sqlx::Error> {
    steer_search_path_statement(search_path, scope).to_query().execute(conn).await?;
    Ok(())
}

/// `SELECT current_setting('search_path')`.
fn current_search_path_statement() -> SelectStatement {
    Query::select().expr(current_setting(Setting::SearchPath)).to_owned()
}

/// `SELECT set_config('search_path', <search_path>, <is_local>)`.
fn steer_search_path_statement(search_path: &SearchPath, scope: SettingScope) -> SelectStatement {
    Query::select().expr(set_config(Setting::SearchPath, search_path.as_str(), scope)).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::search_path::{current_search_path_statement, steer_search_path_statement};
    use andromeda_postgresql_seaquery::{search_path::SearchPath, setting::SettingScope};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_search_path_is_read_as_the_setting() {
        assert_eq!(
            current_search_path_statement().to_string(PostgresQueryBuilder),
            "SELECT current_setting('search_path')"
        );
    }

    /// A transaction scope ends the setting with the transaction; a session
    /// scope keeps it for every later transaction of the connection.
    #[test]
    fn the_search_path_is_set_for_the_scope_asked_for() {
        let path = SearchPath::of(["snap_7", "public"]);
        assert_eq!(
            steer_search_path_statement(&path, SettingScope::Transaction).to_string(PostgresQueryBuilder),
            "SELECT set_config('search_path', 'snap_7, public', TRUE)"
        );
        assert_eq!(
            steer_search_path_statement(&path, SettingScope::Session).to_string(PostgresQueryBuilder),
            "SELECT set_config('search_path', 'snap_7, public', FALSE)"
        );
    }
}
