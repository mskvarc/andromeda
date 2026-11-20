//! The functions of the `seal_conversion` schema the tenant migrations convert
//! data-integrity seals with (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2). The
//! schema exists only while a tenant is between the migration that creates it
//! and the one that drops it.
//!
//! No statement of the broker calls one. A rehearsal of the upgrade calls one
//! in a tenant stopped between those migrations, to hold what the conversion
//! admits against what the broker reads.

use sea_query::{Func, Iden, SimpleExpr};

/// `seal_conversion.admits_uri`, written schema-qualified because the schema is
/// on no search path.
#[derive(Iden, Clone, Copy)]
#[iden = "seal_conversion.admits_uri"]
struct AdmitsUri;

/// Whether the conversion keeps a seal naming the text `uri` as a URI.
#[must_use]
pub fn admits_uri(uri: SimpleExpr) -> SimpleExpr {
    Func::cust(AdmitsUri).arg(uri).into()
}

#[cfg(test)]
mod tests {
    use crate::seal_conversion::admits_uri;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_function_is_called_by_its_schema_qualified_name_with_the_uri() {
        let sql = Query::select()
            .expr(admits_uri(Expr::col(Alias::new("candidate"))))
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"seal_conversion.admits_uri("candidate")"#), "{sql}");
    }
}
