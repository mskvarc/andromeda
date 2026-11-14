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
//! rechecked or probed candidates, or [`RANGED_COUNT_CANDIDATE_BOUND`] + 1
//! from a range of a typed value index, behind a
//! [`one_time_gate`](andromeda_postgresql_query::one_time_gate): when they number at most the
//! bound they are every candidate, and the selection narrowed to them is
//! counted; when more qualify, the selection is walked. A Linked Entity hop's
//! candidates are read whole, as its page reads them.

use crate::{entity_relations::EntityRelations, entity_total::count_over};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    one_time_gate::{COUNT_CANDIDATE_BOUND, CandidateBound, Candidates, column_of, materialized, within},
    q_predicate::keyed_term::CandidateRead,
};
use andromeda_postgresql_schema::current::Entities;
use sea_query::{Condition, DynIden, Expr, ExprTrait, Query, SelectStatement, WithClause};

/// The most candidates a count reads from a range of a typed value index
/// before it walks the selection instead; such a candidate needs no recheck.
pub const RANGED_COUNT_CANDIDATE_BOUND: u64 = 50_000;

/// The most candidates a count reads, by what one costs to read.
pub(crate) const fn count_bound(read: CandidateRead) -> CandidateBound {
    match read {
        CandidateRead::Rechecked | CandidateRead::Probed => CandidateBound::AtMost(COUNT_CANDIDATE_BOUND),
        CandidateRead::Ranged => CandidateBound::AtMost(RANGED_COUNT_CANDIDATE_BOUND),
        CandidateRead::Linked => CandidateBound::Whole,
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
    let narrowed = filters
        .remainder
        .add(rows.entity_key_column().in_subquery(column_of(Candidates, Entities::EntityKey)));
    let indexed = count_over(relations.clone(), schema, narrowed);
    let mut count = Query::select();
    match bound {
        CandidateBound::AtMost(most) => {
            let walked = count_over(relations.clone(), schema, filters.whole);
            count.expr(Expr::case(within(Candidates, most), indexed).finally(walked));
        }
        CandidateBound::Whole => {
            count.expr(indexed);
        }
    }
    count.with_cte(WithClause::new().cte(materialized(bound.read(candidates), Candidates)).to_owned());
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
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse},
        page_window::PageWindow,
    };
    use andromeda_postgresql_query::query_names::QueryNames;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::SelectStatement;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("temperature", 12), ("source", 13), ("hasTrip", 14)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn query(q: &str) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.q = Some(CompiledFilter::new(parse(q).expect("valid q"), LanguageMatching::TagEquality));
        query
    }

    /// A page of 100 past a cursor.
    fn continued() -> PageWindow {
        PageWindow::after(100, "urn:ngsi-ld:Building:001".parse().expect("valid URI"))
    }

    /// The count statement a counted first page runs when it carries no
    /// total.
    fn count_of(q: &str) -> SelectStatement {
        count_on(q, &PageWindow::offset(100, 0))
    }

    fn count_on(q: &str, window: &PageWindow) -> SelectStatement {
        let built = build_query(&ContextTarget::Live, &query(q), window, CountPolicy::Emit, &names()).expect("valid entity query");
        match built.total {
            EntityTotal::OnPageRows { uncounted: count } | EntityTotal::Counted(count) => count.count,
            EntityTotal::Omitted => panic!("a counted query has a total"),
        }
    }

    #[test]
    fn a_keyed_count_reads_its_candidates_under_its_own_bound() {
        assert_statement_snapshot!(
            "keyed-count-a-keyed-count-reads-its-candidates-under-its-own-bound",
            count_of("\"https://example.com/temperature\"~=\"text-71.*\"")
        );
    }

    /// A short walk that fills the page found the term dense, so the count
    /// run then walks the selection without reading a candidate.
    #[test]
    fn a_keyed_page_answered_by_its_short_walk_is_counted_by_the_walk_alone() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        let EntityTotal::OnPageRows { uncounted } = built.total else {
            panic!("a first keyed page carries its total when the candidates answer");
        };
        let dense = uncounted.walked.expect("a keyed page counts a dense term by walking").select;
        assert_statement_snapshot!("keyed-count-a-keyed-page-answered-by-its-short-walk-is-counted-by-the-walk-alone", dense);
    }

    /// Past a cursor the page carries no total, so the count is its own
    /// statement, gated on the candidates the same way.
    #[test]
    fn a_continued_cursor_page_counts_through_the_same_gate() {
        assert_statement_snapshot!(
            "keyed-count-a-continued-cursor-page-counts-through-the-same-gate",
            count_on("\"https://example.com/temperature\"~=\"text-71.*\"", &continued())
        );
    }

    /// A range of the date-time index is a match as it is read, so the count
    /// reads more of them before it walks.
    #[test]
    fn a_date_time_ordering_counts_through_the_ranged_bound() {
        assert_statement_snapshot!(
            "keyed-count-a-date-time-ordering-counts-through-the-ranged-bound",
            count_of("\"https://example.com/temperature\">2024-01-15T12:00:00Z")
        );
    }

    #[test]
    fn a_bare_existence_counts_its_probed_carriers_under_the_rechecked_bound() {
        assert_statement_snapshot!(
            "keyed-count-a-bare-existence-counts-its-probed-carriers-under-the-rechecked-bound",
            count_of("\"https://example.com/temperature\"")
        );
    }

    /// A hop's candidates are read whole, so the count narrows the selection
    /// to them and never walks it outward.
    #[test]
    fn a_linked_entity_hop_counts_every_linking_entity_from_its_candidates() {
        assert_statement_snapshot!(
            "keyed-count-a-linked-entity-hop-counts-every-linking-entity-from-its-candidates",
            count_of("\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\"")
        );
    }

    #[test]
    fn a_keyed_count_counts_the_selection_narrowed_to_few_candidates() {
        assert_statement_snapshot!(
            "keyed-count-a-keyed-count-counts-the-selection-narrowed-to-few-candidates",
            count_of("\"https://example.com/temperature\"~=\"text-71.*\";\"https://example.com/temperature\"!=\"x\"")
        );
    }

    #[test]
    fn a_keyed_count_walks_the_selection_past_its_bound() {
        assert_statement_snapshot!(
            "keyed-count-a-keyed-count-walks-the-selection-past-its-bound",
            count_of("\"https://example.com/temperature\"~=\"text-71.*\"")
        );
    }

    #[test]
    fn a_keyed_sub_attribute_equality_counts_through_the_key_index() {
        assert_statement_snapshot!(
            "keyed-count-a-keyed-sub-attribute-equality-counts-through-the-key-index",
            count_of("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\"")
        );
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
        let EntityTotal::OnPageRows { uncounted } = built.total else {
            panic!("an unkeyed first page carries its total on its rows");
        };
        assert_statement_snapshot!("keyed-count-a-count-without-a-keyed-term-is-one-count-over-the-whole-filter", uncounted.count);
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
        assert!(matches!(built.total, EntityTotal::OnPageRows { .. }));
        assert_eq!(built.total_planning, StatementPlanning::BoundValues);
    }
}
