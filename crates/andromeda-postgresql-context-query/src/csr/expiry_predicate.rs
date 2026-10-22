//! Registration expiry in reads (clause 5.9.2.4).
//!
//! A registration whose `expiresAt` has passed is deleted "when this point in
//! time is reached". The sweeper that deletes the row runs on a cadence, so
//! between the instant and the next sweep the row still exists; every read
//! applies this predicate so the registration is gone from the API the moment
//! it expires, whichever of the two the sweeper reaches first.

use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::clock::now;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait};

/// `expires_at IS NULL OR expires_at > NOW()`: the registrations that have not
/// expired, those with no expiry or one still in the future.
#[must_use]
pub fn not_expired() -> Condition {
    Condition::any()
        .add(SeaExpr::col(Csr::ExpiresAt).is_null())
        .add(SeaExpr::expr(SeaExpr::col(Csr::ExpiresAt)).gt(now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_postgresql_seaquery::literal::int_literal;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    #[test]
    fn an_expired_registration_is_excluded_and_one_without_expiry_is_kept() {
        let sql = Query::select()
            .expr(int_literal(1))
            .from(Alias::new("context_source_registrations"))
            .cond_where(not_expired())
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"expires_at\" IS NULL OR \"expires_at\" > NOW()"), "{sql}");
    }
}
