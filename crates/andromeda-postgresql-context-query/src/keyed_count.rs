//! The total of an entity query whose `q` holds conjuncts an index answers
//! (clause 6.3.13).
//!
//! The total is exact, so the count needs every match, and the two ways to
//! find them cost differently. Read from a term's index, the count costs the
//! candidates, each rechecked or probed and looked up in the selection.
//! Walked, it costs the plan the planner makes of the whole `q`, which reads
//! the term's index itself for a term it believes rare and probes every
//! Entity of the selection for one it does not, and its belief is off by
//! orders of magnitude for these terms. Among 500 000 Entities the index
//! counts 10 000 matches in 5 to 50 ms where the walk takes 12 to 690 ms, and
//! it stays the cheaper way up to 50 000 matches, a tenth of the selection. A
//! pattern whose trigrams most values share is the exception: its index
//! gathers every posting, 350 to 500 ms, which an `idPattern` walks in about
//! 250 ms.
//! The statement therefore reads the conjuncts in the order the page does,
//! each at most [`COUNT_CANDIDATE_BOUND`] + 1 candidates, behind a
//! [`one_time_gate`](andromeda_postgresql_query::one_time_gate): the first
//! read that holds at most its bound holds every candidate of its term, and
//! the selection narrowed to them is counted; when every read finds more, the
//! selection is walked. A Linked Entity hop's candidates are read whole, as
//! its page reads them, so the count never walks past one.

use crate::{entity_relations::EntityRelations, entity_total::count_over};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    one_time_gate::{COUNT_CANDIDATE_BOUND, CandidateBound, Candidates, column_of, materialized, narrowed_to, within},
    q_predicate::keyed_term::CandidateRead,
};
use andromeda_postgresql_schema::current::Entities;
use sea_query::{Condition, DynIden, Expr, Query, SelectStatement, WithClause};

/// The most candidates a count reads: [`COUNT_CANDIDATE_BOUND`] for every
/// read but a hop's, whose candidates are read whole.
pub(crate) const fn count_bound(read: CandidateRead) -> CandidateBound {
    match read {
        CandidateRead::Rechecked | CandidateRead::Ranged | CandidateRead::Probed => CandidateBound::AtMost(COUNT_CANDIDATE_BOUND),
        CandidateRead::Linked => CandidateBound::Whole,
    }
}

/// One read a keyed count tries.
pub(crate) struct CountedRead {
    /// Every filter but the term the candidates answer.
    pub(crate) remainder: Condition,
    /// The candidates, unbounded: `SELECT "entity_key" …`.
    pub(crate) candidates: SelectStatement,
    /// How one candidate is read.
    pub(crate) read: CandidateRead,
}

/// The count of the selection `relations` read in the branch `schema`
/// (`None` for the live branch), from the candidates of the first of `reads`
/// that are few, by walking it under `whole`, every filter, when none are.
pub(crate) fn keyed_count(
    relations: &EntityRelations,
    schema: Option<&DynIden>,
    rows: &EntityRowRefs,
    reads: Vec<CountedRead>,
    whole: Condition,
) -> SelectStatement {
    let mut ctes = WithClause::new();
    let mut arms = Vec::new();
    let mut read_whole = None;
    for (position, read) in reads.into_iter().enumerate() {
        let cte = Candidates::at(position);
        let bound = count_bound(read.read);
        let narrowed = read
            .remainder
            .add(narrowed_to(rows.entity_key_column(), column_of(cte.iden(), Entities::EntityKey)));
        let indexed = count_over(relations.clone(), schema, rows.standing(), narrowed);
        ctes.cte(materialized(bound.read(read.candidates), cte.iden()));
        match bound {
            CandidateBound::AtMost(most) => arms.push((within(cte.iden(), most), indexed)),
            CandidateBound::Whole => {
                read_whole = Some(indexed);
                break;
            }
        }
    }
    let last = read_whole.unwrap_or_else(|| count_over(relations.clone(), schema, rows.standing(), whole));
    let mut arms = arms.into_iter();
    let mut count = Query::select();
    match arms.next() {
        Some((gate, indexed)) => {
            let case = arms.fold(Expr::case(gate, indexed), |case, (gate, indexed)| case.case(gate, indexed));
            count.expr(case.finally(last));
        }
        None => {
            count.expr(last);
        }
    }
    count.with_cte(ctes);
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

    #[test]
    fn a_date_time_ordering_counts_from_a_range_of_its_index() {
        assert_statement_snapshot!(
            "keyed-count-a-date-time-ordering-counts-from-a-range-of-its-index",
            count_of("\"https://example.com/temperature\">2024-01-15T12:00:00Z")
        );
    }

    /// The count reads the conjuncts in the order the page does and counts
    /// from the first whose candidates stay within the count's bound, walking
    /// when none does.
    #[test]
    fn a_conjunction_counts_from_the_first_read_within_its_bound() {
        assert_statement_snapshot!(
            "keyed-count-a-conjunction-counts-from-the-first-read-within-its-bound",
            count_of("\"https://example.com/temperature\"==\"x\";\"https://example.com/source\">1")
        );
    }

    #[test]
    fn a_bare_existence_counts_its_probed_carriers_under_the_count_bound() {
        assert_statement_snapshot!(
            "keyed-count-a-bare-existence-counts-its-probed-carriers-under-the-count-bound",
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
            &query("\"https://example.com/temperature\"!=1"),
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
