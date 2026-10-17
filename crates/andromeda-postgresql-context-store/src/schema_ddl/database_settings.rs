//! `ALTER DATABASE ... SET` session-default DDL.
//!
//! `ALTER DATABASE ... SET` records a database-level default that every future
//! session inherits, so the setting survives a transaction-pooling proxy that
//! binds a different backend to each transaction. A per-connection `SET` cannot:
//! a pooled backend would carry it to the next client that borrows the backend.
//! The statement takes no bound parameters and names the database as a quoted
//! identifier, so it is spelled here rather than built through `SeaQuery`.

/// The `ALTER DATABASE "name" SET <parameter> = '<value>'` statement.
///
/// `name` is embedded as a double-quoted identifier with any embedded quote
/// removed, because an identifier cannot be a bound parameter; callers pass the
/// name `current_database()` reported. `parameter` and `value` are the fixed
/// tokens [`crate::database_defaults::DatabaseDefault`] renders, never caller
/// data.
#[must_use]
pub fn set_database_default(name: &str, parameter: &str, value: &str) -> String {
    format!("ALTER DATABASE \"{}\" SET {parameter} = '{value}'", name.replace('"', ""))
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::database_settings::set_database_default;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_database_name_is_quoted_and_the_value_is_a_literal() {
        assert_eq!(
            set_database_default("andromeda_acme_0001abcd", "synchronous_commit", "on"),
            r#"ALTER DATABASE "andromeda_acme_0001abcd" SET synchronous_commit = 'on'"#
        );
        assert_eq!(
            set_database_default("andromeda_acme_0001abcd", "statement_timeout", "35000ms"),
            r#"ALTER DATABASE "andromeda_acme_0001abcd" SET statement_timeout = '35000ms'"#
        );
    }

    #[test]
    fn an_embedded_quote_cannot_break_out_of_the_identifier() {
        assert_eq!(
            set_database_default(r#"a"b"#, "synchronous_commit", "local"),
            r#"ALTER DATABASE "ab" SET synchronous_commit = 'local'"#
        );
    }
}
