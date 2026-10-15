//! The total of an entity query whose `q` holds a conjunct an index answers
//! (clause 6.3.13).
//!
//! The total is exact, so the count needs every match, and the two ways to
//! find them cost differently. Read from the term's index, the count costs
//! the candidates, each rechecked and looked up in the selection; walked, it
//! costs the selection, each Entity probed for the whole `q`. The index wins
//! by orders of magnitude while the candidates are few against the selection
//! and loses once they approach it, and the planner cannot tell the two
//! apart. The statement therefore reads at most [`COUNT_CANDIDATE_BOUND`] + 1
//! rechecked or probed candidates, or [`RANGED_COUNT_CANDIDATE_BOUND`] + 1 from a range
//! of a typed value index, behind a [`one_time_gate`](crate::one_time_gate): when they
//! number at most the bound they are every candidate, and the selection
//! narrowed to them is counted; when more qualify, the selection is walked.

use crate::{
    entity_relations::EntityRelations,
    entity_total::count_over,
    one_time_gate::{Candidates, bounded, column_of, materialized, within},
};
use andromeda_postgresql_query::{entity_row_refs::EntityRowRefs, q_predicate::keyed_term::CandidateRead};
use sea_query::{Condition, DynIden, Expr, ExprTrait, Query, SelectStatement, WithClause};

/// The most rechecked or probed candidates a count reads from a keyed term's
/// index before it walks the selection instead.
pub const COUNT_CANDIDATE_BOUND: u64 = 10_000;

/// The most candidates a count reads from a range of a typed value index
/// before it walks the selection instead; a ranged candidate needs no
/// recheck.
pub const RANGED_COUNT_CANDIDATE_BOUND: u64 = 50_000;

/// The most candidates a count reads, by what one costs to read.
const fn count_bound(read: CandidateRead) -> u64 {
    match read {
        CandidateRead::Rechecked | CandidateRead::Probed => COUNT_CANDIDATE_BOUND,
        CandidateRead::Ranged => RANGED_COUNT_CANDIDATE_BOUND,
    }
}

/// What a keyed total filters the selection by, each way it counts.
pub(crate) struct KeyedCountFilters {
    /// Every filter but the keyed term, which the candidates answer.
    pub(crate) remainder: Condition,
    /// Every filter, the keyed term included, for the walk.
    pub(crate) whole: Condition,
}

/// The count of the selection `relations` read in the branch `schema`
/// (`None` for the live branch), from the keyed term's `candidates`, which
/// cost `read` each, when they are few, by walking otherwise.
pub(crate) fn keyed_count(
    relations: &EntityRelations,
    schema: Option<&DynIden>,
    rows: &EntityRowRefs,
    filters: KeyedCountFilters,
    candidates: SelectStatement,
    read: CandidateRead,
) -> SelectStatement {
    let bound = count_bound(read);
    let narrowed = filters.remainder.add(rows.entity_key_column().in_subquery(column_of(Candidates)));
    // clone: both counts read the same relations
    let indexed = count_over(relations.clone(), schema, narrowed);
    let walked = count_over(relations.clone(), schema, filters.whole);
    let mut count = Query::select();
    count
        .expr(Expr::case(within(Candidates, bound), indexed).finally(walked))
        .with_cte(WithClause::new().cte(materialized(bounded(candidates, bound), Candidates)).to_owned());
    count.take()
}

#[cfg(test)]
mod tests {
    use crate::{entity::build_query, entity_total::EntityTotal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::EntitySelection,
        filter::parser::parse,
        page_window::PageWindow,
    };
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    };
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Value};

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("temperature", 12), ("source", 13)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
        }
    }

    fn query(q: &str) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.q_expr = Some(parse(q).expect("valid q"));
        query
    }

    /// The count statement of a counted first page, with its bound values.
    fn count_of(q: &str) -> (String, Vec<Value>) {
        let built = build_query(&ContextTarget::Live, &query(q), &PageWindow::offset(100, 0), CountPolicy::Emit, &names()).expect("valid entity query");
        match built.total {
            EntityTotal::Counted(count) => {
                let (sql, values) = count.build(PostgresQueryBuilder);
                (sql, values.0)
            }
            EntityTotal::Omitted | EntityTotal::OnPageRows { .. } => panic!("a keyed page's total is counted on its own"),
        }
    }

    /// The two arms of the count's `CASE`: the count over the candidates,
    /// then the walk.
    fn arms(sql: &str) -> (&str, &str) {
        let (_, after_gate) = sql.split_once(" THEN ").unwrap_or_else(|| panic!("a gated count: {sql}"));
        after_gate.split_once(" ELSE ").unwrap_or_else(|| panic!("a walk past the bound: {sql}"))
    }

    #[test]
    fn a_keyed_count_reads_its_candidates_under_its_own_bound() {
        let (sql, values) = count_of("\"https://example.com/temperature\"~=\"text-71.*\"");
        assert!(
            sql.starts_with(r#"WITH "candidates" AS  MATERIALIZED (SELECT "entity_key" FROM (SELECT "ca"."entity_key""#),
            "{sql}"
        );
        assert!(sql.contains(r#"SELECT (CASE WHEN ((SELECT COUNT(*) FROM "candidates") <= $"#), "{sql}");
        assert!(values.contains(&Value::BigUnsigned(Some(10_001))), "{values:?}");
        assert!(values.contains(&Value::BigUnsigned(Some(10_000))), "{values:?}");
    }

    /// A range of the date-time index is a match as it is read, so the count
    /// reads more of them before it walks.
    #[test]
    fn a_date_time_ordering_counts_through_the_ranged_bound() {
        let (sql, values) = count_of("\"https://example.com/temperature\">2024-01-15T12:00:00Z");
        let (candidates, _) = sql.split_once(" SELECT (CASE WHEN ").unwrap_or_else(|| panic!("a gated count: {sql}"));
        assert!(candidates.contains(r#""ca"."value_date_time" > $"#), "{candidates}");
        assert!(values.contains(&Value::BigUnsigned(Some(50_001))), "{values:?}");
        assert!(values.contains(&Value::BigUnsigned(Some(50_000))), "{values:?}");
        let (indexed, walked) = arms(&sql);
        assert!(!indexed.contains("value_date_time"), "the candidates answer the ordering: {indexed}");
        assert!(walked.contains(r#""ca"."value_date_time" > $"#), "the walk probes the whole q: {walked}");
    }

    #[test]
    fn a_bare_existence_counts_its_probed_carriers_under_the_rechecked_bound() {
        let (sql, values) = count_of("\"https://example.com/temperature\"");
        let (candidates, _) = sql.split_once(" SELECT (CASE WHEN ").unwrap_or_else(|| panic!("a gated count: {sql}"));
        assert!(candidates.contains(r#""entity_search"."attribute_name_keys" @> (ARRAY[12])"#), "{candidates}");
        assert!(values.contains(&Value::BigUnsigned(Some(10_001))), "{values:?}");
        assert!(values.contains(&Value::BigUnsigned(Some(10_000))), "{values:?}");
        let (indexed, walked) = arms(&sql);
        assert!(!indexed.contains("current_attributes"), "the candidates answer the existence: {indexed}");
        assert!(walked.contains(r#"FROM "current_attributes" AS "ca""#), "the walk probes the whole q: {walked}");
    }

    #[test]
    fn a_keyed_count_counts_the_selection_narrowed_to_few_candidates() {
        let (sql, _) = count_of("\"https://example.com/temperature\"~=\"text-71.*\";\"https://example.com/temperature\"!=\"x\"");
        let (indexed, _) = arms(&sql);
        assert!(
            indexed.contains(r#""entities"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "{indexed}"
        );
        assert!(!indexed.contains(r#""ca"."value_string" ~ "#), "the candidates answer the pattern: {indexed}");
        assert!(
            indexed.contains(r#"NOT COALESCE("ca"."value_string" = $"#),
            "the rest of q still filters: {indexed}"
        );
    }

    #[test]
    fn a_keyed_count_walks_the_selection_past_its_bound() {
        let (sql, _) = count_of("\"https://example.com/temperature\"~=\"text-71.*\"");
        let (_, walked) = arms(&sql);
        assert!(!walked.contains("candidates"), "{walked}");
        assert!(walked.contains(r#""ca"."value_string" ~ "#), "the walk probes the whole q: {walked}");
    }

    #[test]
    fn a_keyed_sub_attribute_equality_counts_through_the_key_index() {
        let (sql, _) = count_of("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\"");
        let (candidates, _) = sql.split_once(" SELECT (CASE WHEN ").unwrap_or_else(|| panic!("a gated count: {sql}"));
        assert!(
            candidates.contains(r#"sub_eq_keys("ca"."attribute_name_key", "ca"."sub_attributes") && "#),
            "{candidates}"
        );
        let (indexed, walked) = arms(&sql);
        assert!(!indexed.contains("sub_eq_keys") && !indexed.contains("UNNEST("), "{indexed}");
        assert!(walked.contains("UNNEST("), "{walked}");
    }

    #[test]
    fn a_count_without_a_keyed_term_is_one_count_over_the_whole_filter() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\">1"),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        let EntityTotal::OnPageRows { empty_page } = built.total else {
            panic!("an unkeyed first page carries its total on its rows");
        };
        let sql = empty_page.to_string(PostgresQueryBuilder);
        assert!(sql.starts_with("SELECT COUNT(*) FROM "), "{sql}");
        assert!(!sql.contains("candidates") && !sql.contains("CASE"), "{sql}");
        assert_eq!(built.total_planning, StatementPlanning::Cached);
    }

    #[test]
    fn a_keyed_count_is_planned_with_its_bound_values() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        assert!(matches!(built.total, EntityTotal::Counted(_)));
        assert_eq!(built.total_planning, StatementPlanning::BoundValues);
    }
}
