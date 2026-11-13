//! Whether a stored Entity or attribute instance still stands (clause 4.22).
//!
//! Clause 4.22 makes an Entity, Property or Relationship "invalid" once its
//! `expiresAt` has passed, while letting the deletion that removes it lag
//! ("final deletion will always lag the expiresAt timestamp to a certain
//! extent"). Every read of a stored Entity or instance therefore applies this
//! one predicate to its own expiry column, so a past-due one neither matches,
//! counts, orders nor is traversed in the window before the expiry sweep
//! deletes it.

use andromeda_postgresql_seaquery::clock::now;
use sea_query::{Cond, Expr as SeaExpr, ExprTrait, SimpleExpr};

/// `expires_at IS NULL OR expires_at > NOW()`: the Entity or instance never
/// expires, or its `expiresAt` is still ahead of the transaction's instant.
#[must_use]
pub fn unexpired(expires_at: SimpleExpr) -> SimpleExpr {
    Cond::any()
        .add(expires_at.clone().is_null()) // clone: the column is read twice, for its presence and its value
        .add(SeaExpr::expr(expires_at).gt(now()))
        .into()
}

#[cfg(test)]
mod tests {
    use crate::expiry::unexpired;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn a_row_stands_while_it_never_expires_or_its_expiry_is_ahead() {
        let sql = Query::select()
            .expr(unexpired(Expr::col(("ca", "expires_at"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT "ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW()"#);
    }
}
