//! The page of an entity query whose `q` holds a conjunct an index answers:
//! an equality on a sub-attribute, a `~=` on an attribute's own value, an
//! ordering or a range on an attribute's own `DateTime` or `Date` value, the
//! bare existence of an attribute, or a Linked Entity hop.
//!
//! The planner cannot price either term (it underestimates a broad one by
//! orders of magnitude), so the page is a [`gated_page`]:
//!
//! 1. the short walk probes the first [`SHORT_WALK`] Entities of the
//!    selection in page order for the whole `q`;
//! 2. the index reads at most [`CANDIDATE_BOUND`] + 1 rows through
//!    `idx_ca_sub_eq_keys` or the trigram indexes, each rechecked, at most
//!    [`RANGED_CANDIDATE_BOUND`] + 1 from a range of a typed value index, or
//!    at most [`PROBED_CANDIDATE_BOUND`] + 1 carriers of an attribute name,
//!    each probed for a standing instance, or every Entity naming a linked
//!    entity the hop's inner term keeps
//!    ([`keyed_candidates`](andromeda_postgresql_query::q_predicate::keyed_term::keyed_candidates)),
//!    and the page is cut from the selection narrowed to them under the rest
//!    of `q`;
//! 3. the full walk probes each Entity of the selection for the whole `q`,
//!    when more candidates than the bound qualify. A hop has no full walk.

use crate::{
    entity::QFilter,
    entity_page::ProjectedPage,
    gated_page::{GatedColumns, IndexOrWalk, PageSource, ShortWalk, gated_page, walked_rows},
};
use andromeda_ngsi_ld_query::{filter::compiled_filter::CompiledFilter, read_order::ReadOrder};
use andromeda_postgresql_query::{
    error::RenderError,
    one_time_gate::CandidateBound,
    q_predicate::{
        instance_scan::InstanceScan,
        keyed_term::{CandidateRead, KeyedCandidates},
        projection::compile,
    },
};

/// How many Entities the short walk probes before the index is read.
pub const SHORT_WALK: u32 = 200;

/// The most rechecked candidates the index path hands to the page.
pub const CANDIDATE_BOUND: u64 = 1000;

/// The most candidates a range of a typed value index hands to the page.
///
/// A ranged candidate costs an index entry and a heap visit, with no recheck,
/// so the index answers a term whose matches cluster late in page order far
/// sooner than the walk well past the rechecked bound.
pub const RANGED_CANDIDATE_BOUND: u64 = 50_000;

/// The most carriers of an attribute name the index path hands to the page.
///
/// A carrier costs a posting of the GIN over the search row's names and one
/// probe of the grain constraint, far less than a value recheck, so the bound
/// sits well past the rechecked one: an Entity that holds the attribute among
/// a selection mostly without it is reached through the index, not the walk.
pub const PROBED_CANDIDATE_BOUND: u64 = 10_000;

/// The most candidates the index path hands to the page, by what one costs to
/// read.
///
/// A Linked Entity hop's candidates are read whole. Each costs an entry of
/// the object's index and a heap visit, read from the linked entities its
/// inner term keeps through that term's own index, so the read grows with the
/// matches alone. The outward walk has no such bound: the planner estimates
/// the linked entities an inner term keeps from statistics that rarely hold
/// their values, and believing a few, it probes every Relationship instance
/// once per linked entity, which with thousands of them never finishes.
pub(crate) const fn page_bound(read: CandidateRead) -> CandidateBound {
    match read {
        CandidateRead::Rechecked => CandidateBound::AtMost(CANDIDATE_BOUND),
        CandidateRead::Ranged => CandidateBound::AtMost(RANGED_CANDIDATE_BOUND),
        CandidateRead::Probed => CandidateBound::AtMost(PROBED_CANDIDATE_BOUND),
        CandidateRead::Linked => CandidateBound::Whole,
    }
}

/// The projected page of `source` for a `q` with a keyed term, and the order
/// its rows come back in, its rows carrying what `columns` asks for.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn keyed_page(
    source: &PageSource<'_>,
    q: &CompiledFilter,
    keyed: KeyedCandidates<'_>,
    columns: GatedColumns,
) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let walked = walked_rows(source.schema);
    let short = ShortWalk {
        source: *source,
        walked: source.condition(QFilter::Omitted)?,
        probe: compile(q.lowered(), &InstanceScan::Current(&walked), source.names),
        length: SHORT_WALK,
    };
    let rest = IndexOrWalk {
        candidates: keyed.select,
        bound: page_bound(keyed.read),
        narrowed: source.condition(QFilter::Except(keyed.term))?,
        whole: source.condition(QFilter::Whole)?,
    };
    gated_page(source, short, rest, columns)
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

    /// The page tries the short walk, then the bounded index, then the full
    /// walk, each gated on what the one before it found.
    #[test]
    fn a_keyed_equality_walks_short_then_probes_the_index() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-keyed-equality-walks-short-then-probes-the-index", built.select);
    }

    /// Clause 6.3.13: the index branch reads every candidate before it cuts
    /// the window, so its rows carry the total; the short walk sees part of
    /// the selection and the full walk stops at the page, so theirs carry
    /// none and the count runs on its own.
    #[test]
    fn a_counted_keyed_page_carries_its_total_on_the_index_branch_alone() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        assert!(matches!(built.total, EntityTotal::OnPageRows { .. }), "the page carries the total it can");
        assert_statement_snapshot!("keyed-page-a-counted-keyed-page-carries-its-total-on-the-index-branch-alone", built.select);
    }

    #[test]
    fn an_uncounted_or_continued_keyed_page_carries_no_total() {
        let q = query("\"https://example.com/temperature\"~=\"text-71.*\"");
        let uncounted = build_query(&ContextTarget::Live, &q, &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
        assert_statement_snapshot!("keyed-page-an-uncounted-or-continued-keyed-page-carries-no-total-1", uncounted.select);
        let continued = build_query(
            &ContextTarget::Live,
            &q,
            &PageWindow::after(100, "urn:ngsi-ld:Building:001".parse().expect("valid URI")),
            CountPolicy::Emit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-an-uncounted-or-continued-keyed-page-carries-no-total-2", continued.select);
        assert!(matches!(continued.total, EntityTotal::Counted(_)), "past a cursor the total is its own count");
    }

    #[test]
    fn the_page_reads_one_candidate_past_its_bound() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-the-page-reads-one-candidate-past-its-bound", built.select);
    }

    /// A range of a typed value index is a match as it is read, so the page
    /// reads more of them before it walks.
    #[test]
    fn a_date_time_ordering_reads_its_candidates_under_the_ranged_bound() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\">2024-01-15T12:00:00Z"),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_statement_snapshot!("keyed-page-a-date-time-ordering-reads-its-candidates-under-the-ranged-bound", built.select);
    }

    /// Clause 4.9 `q=attr`: the carriers of the name are each probed for a
    /// standing instance, so the index branch leaves the term out and the full
    /// walk keeps it.
    #[test]
    fn a_bare_existence_pages_through_the_carriers_of_its_name() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_statement_snapshot!("keyed-page-a-bare-existence-pages-through-the-carriers-of-its-name", built.select);
    }

    /// The hop is read from the linked entities inward, every one of them,
    /// and the index branch leaves it out of the filter it applies to the
    /// candidates; the page has no full walk to probe it outward.
    #[test]
    fn a_linked_entity_hop_pages_through_the_instances_naming_its_linked_matches() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_statement_snapshot!(
            "keyed-page-a-linked-entity-hop-pages-through-the-instances-naming-its-linked-matches",
            built.select
        );
    }

    /// A Date is held as its fixed-width text, so its candidates range over the
    /// string index, and the index branch leaves the ordering out.
    #[test]
    fn a_date_ordering_pages_through_the_string_index() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"<=2024-01-15"),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-date-ordering-pages-through-the-string-index", built.select);
    }

    /// The index branch answers the keyed term with its candidates, so its
    /// filter leaves that term out and keeps the others.
    #[test]
    fn the_index_branch_leaves_out_the_term_its_candidates_answer() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\";\"https://example.com/temperature\">1"),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-the-index-branch-leaves-out-the-term-its-candidates-answer", built.select);
    }

    /// A `~=` on an own value tries the short walk, then the trigram
    /// candidates, then the full walk, like a keyed equality.
    #[test]
    fn a_pattern_on_an_own_value_walks_short_then_probes_the_trigram_indexes() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\";\"https://example.com/temperature\"!=\"x\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_statement_snapshot!("keyed-page-a-pattern-on-an-own-value-walks-short-then-probes-the-trigram-indexes", built.select);
    }

    /// Every branch cuts the same window over the same order, so whichever
    /// gate opens returns the page the plain statement would.
    #[test]
    fn a_pattern_page_cuts_the_same_window_as_the_plain_statement() {
        let q = "\"https://example.com/temperature\"~=\"text-71.*\"";
        let window = PageWindow::offset(20, 40);
        let keyed = build_query(&ContextTarget::Live, &query(q), &window, CountPolicy::Omit, &names()).expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-pattern-page-cuts-the-same-window-as-the-plain-statement", keyed.select);
    }

    /// The index branch answers the pattern with its candidates, so its filter
    /// leaves the pattern out, and the full walk keeps it.
    #[test]
    fn the_pattern_candidates_are_rechecked_by_the_term() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"~=\"text-71.*\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-the-pattern-candidates-are-rechecked-by-the-term", built.select);
    }

    /// A pattern on a sub-attribute has no key, so the page is the plain walk.
    #[test]
    fn a_sub_attribute_pattern_pages_without_the_index() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"~=\"^sensor-4[0-9]$\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-sub-attribute-pattern-pages-without-the-index", built.select);
    }

    /// Without the operand a generic plan cannot estimate the key overlap and
    /// reads the attribute's rows through its name index instead, so the
    /// keyed page is planned with its bound values, and its gated-off
    /// branches launch no parallel worker.
    #[test]
    fn a_keyed_page_is_planned_with_its_bound_values_and_runs_serially() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\".\"https://example.com/source\"==\"uncalibrated\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
    }

    #[test]
    fn a_page_without_a_keyed_term_takes_any_cached_plan() {
        for q in [
            "\"https://example.com/temperature\".\"https://example.com/source\"~=\"^sensor-4[0-9]$\"",
            "\"https://example.com/temperature\">1",
        ] {
            let built = build_query(&ContextTarget::Live, &query(q), &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            assert_eq!(built.planning, StatementPlanning::Cached, "{q}");
        }
    }
}
