//! The PostgreSQL spelling of clause-4.23 Entity ordering.
//!
//! Three things are decided here because every statement that orders entities
//! decides them the same way: which SQL sort order a direction is, which
//! collation object a comparison runs under when the request named none, and
//! how the `id` member is turned into a sort key. The entity query orders by
//! `id` among other members (clause 4.23.3); a temporal query orders by `id`
//! and nothing else (clause 5.7.4.3). Both reach the same key through here.

use crate::sort_key::{KeyOrder, NullPlacement, SortKey};
use andromeda_ngsi_ld_query::ordering::{collation::IcuCollation, sort_direction::SortDirection};
use andromeda_postgresql_seaquery::collation::{PostgresCollation, collate};
use sea_query::{Order, SimpleExpr};

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
/// the column happens to declare. The identifier columns are `COLLATE "C"`,
/// which is byte-wise and is not the clause's default.
#[must_use]
pub fn ordering_collation(requested: Option<&IcuCollation>) -> PostgresCollation {
    PostgresCollation::new(requested.map_or(IcuCollation::ROOT, IcuCollation::as_str))
}

/// The sort key that orders entities by their `id` member.
///
/// `NULLS LAST` states clause 4.23.2's "null values last" for this key, in both
/// directions. An entity identifier is never null, so the ordering is the same
/// with or without it on the entity path; it is written all the same, because
/// the clause pins the position of an absent value rather than leaving it to
/// PostgreSQL's direction-dependent default.
#[must_use]
pub fn entity_identifier_key(identifier: SimpleExpr, direction: SortDirection, collation: &PostgresCollation) -> SortKey {
    SortKey::new(collate(identifier, collation), KeyOrder::new(direction, NullPlacement::Last))
}

#[cfg(test)]
mod tests {
    use crate::entity_ordering::{entity_identifier_key, ordering_collation, sql_order};
    use andromeda_ngsi_ld_query::ordering::{collation::IcuCollation, sort_direction::SortDirection};
    use andromeda_postgresql_seaquery::collation::PostgresCollation;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, Order, PostgresQueryBuilder, Query};

    fn ordered_by_identifier(collation: Option<&IcuCollation>, direction: SortDirection) -> String {
        let mut select = Query::select();
        select.expr(Expr::col(Alias::new("id")));
        entity_identifier_key(Expr::col(Alias::new("id")), direction, &ordering_collation(collation)).order_by(&mut select);
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
        assert_eq!(ordering_collation(None), PostgresCollation::new("und-x-icu"));
    }

    #[test]
    fn a_named_collation_is_the_one_the_request_asked_for() {
        let requested = IcuCollation::parse("de-u-co-phonebk").expect("valid collation");
        assert_eq!(ordering_collation(Some(&requested)), PostgresCollation::new("de-u-co-phonebk"));
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
