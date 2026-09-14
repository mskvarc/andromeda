//! The PostgreSQL spelling of clause-4.23 Entity ordering.
//!
//! Three things are decided here because every statement that orders entities
//! decides them the same way: which SQL sort order a direction is, which
//! collation object a comparison runs under when the request named none, and
//! how the `id` member is turned into a sort key. The entity query orders by
//! `id` among other members (clause 4.23.3); a temporal query orders by `id`
//! and nothing else (clause 5.7.4.3). Both reach the same key through here.

use andromeda_ngsi_ld_query::ordering::{collation::IcuCollation, sort_direction::SortDirection};
use andromeda_postgresql_seaquery::collation::{PostgresCollation, collate};
use sea_query::{NullOrdering, Order, SelectStatement, SimpleExpr};

/// The SQL sort order one clause-4.23 direction runs in.
#[must_use]
pub const fn sql_order(direction: SortDirection) -> Order {
    match direction {
        SortDirection::Ascending => Order::Asc,
        SortDirection::Descending => Order::Desc,
    }
}

/// The collation a clause-4.23 ordering compares strings under.
///
/// Clause 4.23.1: "for strings the default collation order shall be defined as
/// using ICU "root" collation order ("und-x-icu")". A request that named no
/// collation therefore still sorts under a named collation, not under whatever
/// the column happens to declare -- the identifier columns are `COLLATE "C"`,
/// which is byte-wise and is not the clause's default.
#[must_use]
pub fn ordering_collation(requested: Option<&IcuCollation>) -> PostgresCollation {
    PostgresCollation::new(requested.map_or(IcuCollation::ROOT, IcuCollation::as_str))
}

/// Add the sort key that orders entities by their `id` member to `select`.
///
/// `NULLS LAST` states clause 4.23.2's "null values last" for this key, in both
/// directions. An entity identifier is never null, so the ordering is the same
/// with or without it on the entity path; it is written all the same, because
/// the clause pins the position of an absent value rather than leaving it to
/// PostgreSQL's direction-dependent default.
pub fn order_by_entity_identifier(select: &mut SelectStatement, identifier: SimpleExpr, direction: SortDirection, collation: &PostgresCollation) {
    select.order_by_expr_with_nulls(collate(identifier, collation), sql_order(direction), NullOrdering::Last);
}

#[cfg(test)]
mod tests {
    use crate::entity_ordering::{order_by_entity_identifier, ordering_collation, sql_order};
    use andromeda_ngsi_ld_query::ordering::{collation::IcuCollation, sort_direction::SortDirection};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, Order, PostgresQueryBuilder, Query};

    fn ordered_by_identifier(collation: Option<&IcuCollation>, direction: SortDirection) -> String {
        let mut select = Query::select();
        select.expr(Expr::col(Alias::new("id")));
        order_by_entity_identifier(&mut select, Expr::col(Alias::new("id")), direction, &ordering_collation(collation));
        select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn each_direction_renders_its_own_sql_order() {
        assert_eq!(sql_order(SortDirection::Ascending), Order::Asc);
        assert_eq!(sql_order(SortDirection::Descending), Order::Desc);
    }

    /// Clause 4.23.1 names `und-x-icu` as the default collation order, so a
    /// request that named none still compares under it.
    #[test]
    fn an_unnamed_collation_falls_back_to_the_icu_root_order() {
        assert_eq!(ordering_collation(None).as_str(), "und-x-icu");
    }

    #[test]
    fn a_named_collation_is_the_one_the_request_asked_for() {
        let requested = IcuCollation::parse("de-u-co-phonebk").expect("valid collation");
        assert_eq!(ordering_collation(Some(&requested)).as_str(), "de-u-co-phonebk");
    }

    #[test]
    fn the_identifier_key_collates_and_puts_absent_values_last() {
        let requested = IcuCollation::parse("de-u-co-phonebk").expect("valid collation");
        let sql = ordered_by_identifier(Some(&requested), SortDirection::Ascending);
        assert!(sql.contains(r#"("id") COLLATE "de-u-co-phonebk" ASC NULLS LAST"#), "{sql}");
    }

    #[test]
    fn a_descending_identifier_key_reverses_only_the_order() {
        let sql = ordered_by_identifier(None, SortDirection::Descending);
        assert!(sql.contains(r#"("id") COLLATE "und-x-icu" DESC NULLS LAST"#), "{sql}");
    }
}
