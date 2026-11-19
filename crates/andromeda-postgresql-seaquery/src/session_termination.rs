//! Ending the current role's other idle sessions in the current database.

use crate::introspection::current_database;
use sea_query::{Alias, Expr, ExprTrait, Func, Query, SelectStatement};

/// `SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname =
/// current_database() AND pid <> pg_backend_pid() AND usename = current_user
/// AND state = 'idle' AND backend_type = 'client backend'`, one row per
/// session it ends, `true` for each one the server signalled.
///
/// The call sits in the select list, so it runs only for the rows the filter
/// keeps. A session that is running a statement or holds a transaction open is
/// left to finish it.
#[must_use]
pub fn idle_sessions_termination() -> SelectStatement {
    Query::select()
        .expr(Func::cust(Alias::new("pg_terminate_backend")).arg(Expr::col(Alias::new("pid"))))
        .from(Alias::new("pg_stat_activity"))
        .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
        .and_where(Expr::col(Alias::new("pid")).ne(Func::cust(Alias::new("pg_backend_pid"))))
        .and_where(Expr::col(Alias::new("usename")).eq(Expr::cust("current_user")))
        .and_where(Expr::col(Alias::new("state")).eq("idle"))
        .and_where(Expr::col(Alias::new("backend_type")).eq("client backend"))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::session_termination::idle_sessions_termination;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn only_the_idle_client_sessions_of_the_current_role_in_the_current_database_are_ended() {
        assert_eq!(
            idle_sessions_termination().to_string(PostgresQueryBuilder),
            r#"SELECT pg_terminate_backend("pid") FROM "pg_stat_activity" WHERE "datname" = current_database() AND "pid" <> pg_backend_pid() AND "usename" = (current_user) AND "state" = 'idle' AND "backend_type" = 'client backend'"#
        );
    }
}
