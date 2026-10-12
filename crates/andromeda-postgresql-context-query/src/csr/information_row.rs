//! The correlated test over one registration's `registration_information` rows.
//!
//! Each row is one `EntityInfo` of an information item (or the one row of an
//! item without `entities`) and carries its item's attribute names. Clause 5.12
//! matches an entity specification and an attribute list against the same
//! `RegistrationInfo`, and an `EntityInfo` decides its type and its identifier
//! together, so every registration predicate over `information` is one
//! `EXISTS` over the registration's rows with all of its axes on one row.

use andromeda_postgresql_schema::csr::{Csr, RegistrationInformation};
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Query, SimpleExpr};

/// The alias every statement reads `registration_information` under.
#[must_use]
pub fn ri() -> Alias {
    Alias::new("ri")
}

/// A `"ri"."column"` reference into the correlated scan.
#[must_use]
pub fn ri_col(column: RegistrationInformation) -> SimpleExpr {
    SeaExpr::col((ri(), column))
}

/// `EXISTS (SELECT 1 FROM registration_information AS ri WHERE
/// ri.registration_id = context_source_registrations.id AND <row_matches>)`.
///
/// The correlation names the unaliased `context_source_registrations`, which
/// every registration statement selects from under its own name.
#[must_use]
pub fn some_information_row(row_matches: Condition) -> SimpleExpr {
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_as(RegistrationInformation::Table, ri())
        .and_where(ri_col(RegistrationInformation::RegistrationId).eq(SeaExpr::col((Csr::Table, Csr::Id))))
        .cond_where(row_matches);
    SeaExpr::exists(scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_scan_is_correlated_with_the_registration_and_carries_the_row_condition() {
        let sql = Query::select()
            .expr(int_literal(1))
            .from(Csr::Table)
            .and_where(some_information_row(Condition::all().add(ri_col(RegistrationInformation::EntityId).is_null())))
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(
                "EXISTS(SELECT 1 FROM \"registration_information\" AS \"ri\" WHERE \"ri\".\"registration_id\" = \"context_source_registrations\".\"id\" AND \"ri\".\"entity_id\" IS NULL)"
            ),
            "{sql}"
        );
    }
}
