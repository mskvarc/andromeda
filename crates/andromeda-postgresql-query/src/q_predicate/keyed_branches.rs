//! The candidate statement of a term on an attribute's own value whose
//! condition is a disjunction no one index drives: one branch per place the
//! value can be held, each a read of the attribute's standing rows through
//! that place's index, put together as a `UNION ALL`.
//!
//! The branches together keep exactly the rows the disjunction keeps, so the
//! page can leave the term out of the filter it applies to the candidates, and
//! a bounded read stops at the first branches that fill the bound.
//!
//! A term a B-tree yields in index order also has its index entries, the same
//! branches over each typed field's own columns, standing or not, which bound
//! the candidates from above. An index-only scan counts them without visiting
//! the heap, so a read of them bounded at a thousand stops after as many index
//! entries, whatever PostgreSQL estimates of the term. Read for its rows, the
//! term's estimate decides the plan, and a term a hundred thousand rows hold
//! but estimated at twelve thousand is read through a bitmap of every one of
//! them, 10 ms where its entries count to a thousand in 0.2 ms.

use crate::{
    entity_row_refs::EntityRowRefs,
    q_predicate::projection_probe::{Ca, of_attribute, projection_column, standing},
};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::literal::int_literal;
use andromeda_postgresql_stored_values::{attribute_kind::AttributeKind, attribute_name_key::AttributeNameKey};
use sea_query::{Alias, Asterisk, Condition, ExprTrait, Query, SelectStatement, SimpleExpr, UnionType};

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
        .and_where(standing(rows));
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

/// One branch of a term's index entries, `SELECT 1` over the rows of
/// `attribute`, standing or not, that `held` reaches and `keys` keeps.
#[must_use]
pub(crate) fn entry(rows: &EntityRowRefs, attribute: AttributeNameKey, held: Option<SimpleExpr>, keys: Condition) -> SelectStatement {
    let mut entry = Query::select();
    entry
        .expr(int_literal(1))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .and_where(of_attribute(attribute));
    if let Some(held) = held {
        entry.and_where(held);
    }
    entry.cond_where(keys);
    entry.take()
}

/// The candidates of a term a B-tree yields in index order, and the index
/// entries that bound them.
#[derive(Debug, Clone)]
pub(crate) struct BoundedCandidates {
    /// `SELECT "entity_key"` over every candidate.
    pub(crate) candidates: SelectStatement,
    /// `SELECT *` over the `UNION ALL` of the term's entry branches.
    pub(crate) entries: SelectStatement,
}

/// `SELECT *` over the `UNION ALL` of the entry branches `entries`, `None`
/// when there are none.
#[must_use]
pub(crate) fn entries_of(entries: impl IntoIterator<Item = SelectStatement>) -> Option<SelectStatement> {
    let mut entries = entries.into_iter();
    let mut union = entries.next()?;
    for entry in entries {
        union.union(UnionType::All, entry);
    }
    let mut read = Query::select();
    read.column(Asterisk).from_subquery(union, Alias::new(MATCHES));
    Some(read.take())
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
        entity_row_refs::EntityRowRefs,
        q_predicate::keyed_branches::{branch, entries_of, entry, language_property_rows, union_of},
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
    use sea_query::{Condition, SelectStatement, SimpleExpr};

    fn rows_branch(held: Option<SimpleExpr>) -> SelectStatement {
        branch(&EntityRowRefs::unaliased(None), AttributeNameKey::from(4), held, Condition::all())
    }

    #[test]
    fn a_branch_reads_the_standing_rows_of_its_attribute() {
        assert_statement_snapshot!("keyed-branches-a-branch-reads-the-standing-rows-of-its-attribute", rows_branch(None));
    }

    #[test]
    fn a_language_branch_names_the_language_property_rows() {
        assert_statement_snapshot!(
            "keyed-branches-a-language-branch-names-the-language-property-rows",
            rows_branch(Some(language_property_rows()))
        );
    }

    #[test]
    fn the_branches_are_read_as_one_union_all() {
        let statement = union_of([rows_branch(None), rows_branch(None), rows_branch(None)]).expect("three branches");
        assert_statement_snapshot!("keyed-branches-the-branches-are-read-as-one-union-all", statement);
    }

    #[test]
    fn no_branch_reads_no_candidates() {
        assert!(union_of([]).is_none());
        assert!(entries_of([]).is_none());
    }

    /// An entry reads the attribute's rows whether or not they stand, so it
    /// tests only the columns an index over the field keys.
    #[test]
    fn the_entries_read_every_row_of_the_attribute_by_its_keys() {
        let rows = EntityRowRefs::unaliased(None);
        let entries = entries_of([
            entry(&rows, AttributeNameKey::from(4), None, Condition::all()),
            entry(&rows, AttributeNameKey::from(4), Some(language_property_rows()), Condition::all()),
        ])
        .expect("two entries");
        assert_statement_snapshot!("keyed-branches-the-entries-read-every-row-of-the-attribute-by-its-keys", entries);
    }
}
