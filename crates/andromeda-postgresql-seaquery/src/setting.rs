//! Changing a runtime configuration setting from a statement.
//!
//! `SET` takes no bind parameter, so a setting whose value is computed at run
//! time -- the search path a Snapshot branch migration runs under -- is changed
//! through `set_config(name, value, is_local)`, with both the name and the value
//! bound rather than interpolated.

use sea_query::{Expr, Func, Iden, SimpleExpr};

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

/// `set_config(name, value, is_local)` - change a runtime setting for `scope`.
#[must_use]
pub fn set_config(name: &str, value: &str, scope: SettingScope) -> SimpleExpr {
    let is_local = match scope {
        SettingScope::Transaction => true,
        SettingScope::Session => false,
    };
    Func::cust(SetConfig)
        .args([
            SimpleExpr::from(Expr::val(name.to_owned())),
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
    set_config("statement_timeout", "0", SettingScope::Transaction)
}

#[cfg(test)]
mod tests {
    use crate::setting::{SettingScope, lift_statement_timeout, set_config};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn a_transaction_scoped_setting_is_local_and_binds_its_name_and_value() {
        let (sql, values) = Query::select()
            .expr(set_config("search_path", "snap_7, public", SettingScope::Transaction))
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT set_config($1, $2, $3)");
        assert_eq!(values.0.len(), 3);
        assert!(format!("{:?}", values.0[2]).contains("true"), "{values:?}");
    }

    #[test]
    fn a_session_scoped_setting_is_not_local() {
        let (_, values) = Query::select()
            .expr(set_config("search_path", "snap_7, public", SettingScope::Session))
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
}
