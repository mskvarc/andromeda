//! The candidate statement of a term on an attribute's own value whose
//! condition is a disjunction no one index drives: one branch per place the
//! value can be held, each a read of the attribute's standing rows through
//! that place's index, put together as a `UNION ALL`.
//!
//! The branches together keep exactly the rows the disjunction keeps, so the
//! page can leave the term out of the filter it applies to the candidates, and
//! a bounded read stops at the first branches that fill the bound.

use crate::{
    attribute_name_key::AttributeNameKey,
    entity_row_refs::EntityRowRefs,
    q_predicate::projection_probe::{Ca, of_attribute, projection_column, standing},
};
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Alias, Condition, ExprTrait, Query, SelectStatement, SimpleExpr, UnionType};

/// The name the union of the branches is read under.
const MATCHES: &str = "matches";

/// One branch: `SELECT "ca"."entity_key"` over the standing rows of
/// `attribute` that `held` reaches and `test` keeps.
#[must_use]
pub(crate) fn branch(rows: &EntityRowRefs, attribute: AttributeNameKey, held: Option<SimpleExpr>, test: Condition) -> SelectStatement {
    let mut branch = Query::select();
    branch
        .expr(projection_column(CurrentAttributes::EntityKey))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .and_where(of_attribute(attribute))
        .and_where(standing());
    if let Some(held) = held {
        branch.and_where(held);
    }
    branch.cond_where(test);
    branch.take()
}

/// The rows of a `LanguageProperty`, whose `languageMap` is a `value_object`
/// that `idx_ca_value_object` keys by attribute, stated so the planner reaches
/// them through that index when the map's own test gives it none.
#[must_use]
pub(crate) fn language_property_rows() -> SimpleExpr {
    projection_column(CurrentAttributes::AttributeKind)
        .eq(int_literal(i64::from(AttributeKind::LanguageProperty.code())))
        .and(projection_column(CurrentAttributes::ValueObject).is_not_null())
}

/// `SELECT "entity_key"` over the `UNION ALL` of `branches`, `None` when
/// there are none.
#[must_use]
pub(crate) fn union_of(branches: impl IntoIterator<Item = SelectStatement>) -> Option<SelectStatement> {
    let mut branches = branches.into_iter();
    let mut union = branches.next()?;
    for branch in branches {
        union.union(UnionType::All, branch);
    }
    let mut candidates = Query::select();
    candidates.column(CurrentAttributes::EntityKey).from_subquery(union, Alias::new(MATCHES));
    Some(candidates.take())
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        entity_row_refs::EntityRowRefs,
        q_predicate::keyed_branches::{branch, language_property_rows, union_of},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Condition, PostgresQueryBuilder, SelectStatement, SimpleExpr};

    fn rows_branch(held: Option<SimpleExpr>) -> SelectStatement {
        branch(&EntityRowRefs::unaliased(None), AttributeNameKey::from(4), held, Condition::all())
    }

    #[test]
    fn a_branch_reads_the_standing_rows_of_its_attribute() {
        let sql = rows_branch(None).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "ca"."entity_key" FROM "current_attributes" AS "ca" WHERE "ca"."attribute_name_key" = 4 AND ("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW())"#
        );
    }

    #[test]
    fn a_language_branch_names_the_language_property_rows() {
        let sql = rows_branch(Some(language_property_rows())).to_string(PostgresQueryBuilder);
        assert!(
            sql.ends_with(r#"AND ("ca"."attribute_kind" = (5) AND "ca"."value_object" IS NOT NULL)"#),
            "{sql}"
        );
    }

    #[test]
    fn the_branches_are_read_as_one_union_all() {
        let sql = union_of([rows_branch(None), rows_branch(None), rows_branch(None)])
            .expect("three branches")
            .to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT "entity_key" FROM (SELECT "ca"."entity_key""#), "{sql}");
        assert_eq!(sql.matches("UNION ALL").count(), 2, "{sql}");
        assert!(sql.ends_with(r#") AS "matches""#), "{sql}");
    }

    #[test]
    fn no_branch_reads_no_candidates() {
        assert!(union_of([]).is_none());
    }
}
