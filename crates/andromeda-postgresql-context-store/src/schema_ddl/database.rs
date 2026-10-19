//! `CREATE DATABASE` and `DROP DATABASE` DDL.
//!
//! Neither statement can run inside a transaction or take bound parameters, so it is neither a `SeaQuery` statement nor executed through
//! `query_with`; the database name is a quoted identifier, not a value.

/// The `CREATE DATABASE "name"` statement for `name`.
///
/// The name is embedded as a double-quoted identifier with any embedded quote
/// removed, because an identifier cannot be a bound parameter. Callers pass a
/// name already constrained to the sanitised, length-bounded form
/// [`crate::tenant_database::generate_database_name`] produces.
#[must_use]
pub fn create_database(name: &str) -> String {
    format!("CREATE DATABASE \"{}\"", name.replace('"', ""))
}

/// The `DROP DATABASE IF EXISTS "name"` statement for `name`.
///
/// Carries no `FORCE`: PostgreSQL refuses the drop while any session is
/// connected to the database, which is what keeps a database some broker still
/// serves from being dropped underneath it. The name is quoted as
/// [`create_database`] quotes it.
#[must_use]
pub fn drop_database(name: &str) -> String {
    format!("DROP DATABASE IF EXISTS \"{}\"", name.replace('"', ""))
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::database::{create_database, drop_database};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_name_is_quoted_as_an_identifier() {
        assert_eq!(create_database("andromeda_acme_0001abcd"), r#"CREATE DATABASE "andromeda_acme_0001abcd""#);
    }

    #[test]
    fn an_embedded_quote_cannot_break_out_of_the_identifier() {
        assert_eq!(create_database(r#"a"b"#), r#"CREATE DATABASE "ab""#);
    }

    #[test]
    fn a_drop_tolerates_a_missing_database_and_never_forces_out_sessions() {
        assert_eq!(drop_database("andromeda_acme_0001abcd"), r#"DROP DATABASE IF EXISTS "andromeda_acme_0001abcd""#);
        assert_eq!(drop_database(r#"a"b"#), r#"DROP DATABASE IF EXISTS "ab""#);
    }
}
