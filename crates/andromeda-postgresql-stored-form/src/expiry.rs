//! Whether a stored Entity or attribute instance still stands (clause 4.22).
//!
//! Clause 4.22 makes an Entity, Property or Relationship "invalid" once its
//! `expiresAt` has passed, while letting the deletion that removes it lag
//! ("final deletion will always lag the expiresAt timestamp to a certain
//! extent"). Every read of a stored Entity or instance therefore applies this
//! one predicate to its own expiry column, so a past-due one neither matches,
//! counts, orders nor is traversed in the window before the expiry sweep
//! deletes it.
//!
//! The instant a read judges expiry at is the caller's: the transaction's own
//! `NOW()` for a request, or a bound instant an evaluation fixed for every
//! statement it runs.

use sea_query::{Cond, Expr as SeaExpr, ExprTrait, SimpleExpr};

/// `expires_at IS NULL OR expires_at > at`: the Entity or instance never
/// expires, or its `expiresAt` is still ahead of the instant `at`.
#[must_use]
pub fn unexpired(expires_at: SimpleExpr, at: SimpleExpr) -> SimpleExpr {
    Cond::any()
        .add(expires_at.clone().is_null()) // clone: the column is read twice, for its presence and its value
        .add(SeaExpr::expr(expires_at).gt(at))
        .into()
}

#[cfg(test)]
mod tests {
    use crate::expiry::unexpired;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, clock::now};
    use sea_query::{Expr, Query};

    #[test]
    fn a_row_stands_while_it_never_expires_or_its_expiry_is_ahead() {
        let statement = Query::select().expr(unexpired(Expr::col(("ca", "expires_at")), now())).to_owned();
        assert_statement_snapshot!("expiry-a-row-stands-while-it-never-expires-or-its-expiry-is-ahead", statement);
    }
}
