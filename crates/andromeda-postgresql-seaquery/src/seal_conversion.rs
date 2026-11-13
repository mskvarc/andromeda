//! The functions the tenant migrations convert data-integrity seals with
//! (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2), which exist only while a tenant is
//! between the migration that creates them and the one that drops them.
//!
//! No statement of the broker calls one. A rehearsal of the upgrade calls one
//! in a tenant stopped between those migrations, to hold what the conversion
//! admits against what the broker reads.

use sea_query::{Func, Iden, SimpleExpr};

/// `seal_conversion_admits_uri`.
#[derive(Iden, Clone, Copy)]
#[iden = "seal_conversion_admits_uri"]
struct SealConversionAdmitsUri;

/// Whether the conversion keeps a seal naming the text `uri` as a URI.
#[must_use]
pub fn seal_conversion_admits_uri(uri: SimpleExpr) -> SimpleExpr {
    Func::cust(SealConversionAdmitsUri).arg(uri).into()
}

#[cfg(test)]
mod tests {
    use crate::seal_conversion::seal_conversion_admits_uri;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_function_takes_the_uri() {
        let sql = Query::select()
            .expr(seal_conversion_admits_uri(Expr::col(Alias::new("candidate"))))
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"seal_conversion_admits_uri("candidate")"#), "{sql}");
    }
}
