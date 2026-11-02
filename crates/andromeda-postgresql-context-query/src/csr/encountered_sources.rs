//! Loop prevention in registration matching (clauses 4.3.6.4 and 5.12).
//!
//! A distributed operation carries the listing of the context sources it has
//! already passed through. No registration whose `contextSourceAlias` appears
//! in that listing matches, so the request is never forwarded back to a source
//! it came from.

use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::pg_array::is_none_of;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait};

/// `context_source_alias IS NULL OR context_source_alias <> ALL(ARRAY[...])`.
pub(crate) fn not_encountered(encountered: &[ContextSourcePseudonym]) -> Condition {
    Condition::any().add(SeaExpr::col(Csr::ContextSourceAlias).is_null()).add(is_none_of(
        SeaExpr::col(Csr::ContextSourceAlias),
        encountered.iter().map(|alias| alias.as_str().to_owned()),
    ))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use andromeda_postgresql_seaquery::literal::int_literal;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    #[test]
    fn an_aliased_registration_is_excluded_when_its_alias_was_encountered() {
        let encountered = [ContextSourcePseudonym::new("broker-b").expect("valid pseudonym")];
        let sql = Query::select()
            .expr(int_literal(1))
            .from(Alias::new("context_source_registrations"))
            .cond_where(not_encountered(&encountered))
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("\"context_source_alias\" IS NULL OR \"context_source_alias\" <> 'broker-b'"),
            "{sql}"
        );
    }
}
