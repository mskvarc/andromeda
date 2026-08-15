//! `CREATE DATABASE` DDL.
//!
//! `CREATE DATABASE` cannot run inside a transaction and takes no bound
//! parameters, so it is neither a SeaQuery statement nor executed through
//! `query_with`; the database name is a quoted identifier, not a value.

/// The `CREATE DATABASE "name"` statement for `name`.
///
/// The name is embedded as a double-quoted identifier with any embedded quote
/// removed, because an identifier cannot be a bound parameter. Callers pass a
/// name already constrained to the sanitised, length-bounded form
/// [`crate::tenant_database::generate_db_name`] produces.
pub fn create_database(name: &str) -> String {
    format!("CREATE DATABASE \"{}\"", name.replace('"', ""))
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::database::create_database;

    #[test]
    fn the_name_is_quoted_as_an_identifier() {
        assert_eq!(create_database("andromeda_acme_0001abcd"), r#"CREATE DATABASE "andromeda_acme_0001abcd""#);
    }

    #[test]
    fn an_embedded_quote_cannot_break_out_of_the_identifier() {
        assert_eq!(create_database(r#"a"b"#), r#"CREATE DATABASE "ab""#);
    }
}
