//! `ALTER DATABASE ... SET` durability DDL.
//!
//! `ALTER DATABASE ... SET` records a database-level default that every future
//! session inherits, so the setting survives a transaction-pooling proxy that
//! binds a different backend to each transaction. A per-connection `SET` cannot:
//! a pooled backend would carry it to the next client that borrows the backend.
//! The statement takes no bound parameters and names the database as a quoted
//! identifier, so it is spelled here rather than built through `SeaQuery`.

/// The `ALTER DATABASE "name" SET synchronous_commit = '<setting>'` statement.
///
/// `name` is embedded as a double-quoted identifier with any embedded quote
/// removed, because an identifier cannot be a bound parameter; callers pass a
/// name already constrained to the sanitised, length-bounded form
/// [`crate::tenant_database::generate_database_name`] produces, or the platform
/// database name read from the connection URL. `setting` is one of the fixed
/// PostgreSQL tokens [`crate::config::SynchronousCommit::as_postgres_setting`]
/// renders.
#[must_use]
pub fn set_database_synchronous_commit(name: &str, setting: &str) -> String {
    format!("ALTER DATABASE \"{}\" SET synchronous_commit = '{setting}'", name.replace('"', ""))
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::database_settings::set_database_synchronous_commit;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_database_name_is_quoted_and_the_setting_is_a_literal() {
        assert_eq!(
            set_database_synchronous_commit("andromeda_acme_0001abcd", "off"),
            r#"ALTER DATABASE "andromeda_acme_0001abcd" SET synchronous_commit = 'off'"#
        );
    }

    #[test]
    fn an_embedded_quote_cannot_break_out_of_the_identifier() {
        assert_eq!(
            set_database_synchronous_commit(r#"a"b"#, "local"),
            r#"ALTER DATABASE "ab" SET synchronous_commit = 'local'"#
        );
    }
}
