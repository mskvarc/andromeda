//! The page of an entity query whose `q` holds conjuncts an index answers:
//! an equality with one operand on an attribute's own value or on a
//! sub-attribute, a `~=` on an attribute's own value, an ordering or a range
//! on an attribute's own number, `DateTime`, `Date` or `Time` value,
//! the bare existence of an attribute, or a Linked Entity hop.
//!
//! The planner cannot price any of these terms (it underestimates a broad one
//! by orders of magnitude), so the page is a [`gated_page`]:
//!
//! 1. the short walk probes the first [`SHORT_WALK`] Entities of the
//!    selection in page order for the whole `q`;
//! 2. the index reads each keyed conjunct in turn
//!    ([`keyed_candidates`](andromeda_postgresql_query::q_predicate::keyed_term::keyed_candidates)):
//!    at most [`CANDIDATE_BOUND`] + 1 rows through the value and containment
//!    indexes, `idx_ca_sub_eq_keys` or the trigram indexes, each rechecked,
//!    from a range of a typed value index, or as carriers of an attribute
//!    name, each probed for a standing instance, or every Entity naming a
//!    linked entity the hop's inner term keeps. The page is cut from the
//!    selection narrowed to the first candidates within their bound, under
//!    the rest of `q`;
//! 3. the full walk probes each Entity of the selection in page order for the
//!    whole `q`, when every read found more candidates than its bound. A hop's
//!    candidates are read whole, so no read follows it and the page has no
//!    full walk.

use crate::{
    entity::{GeoFilter, QFilter},
    entity_page::ProjectedPage,
    gated_page::{GatedColumns, IndexRead, PageSource, PageWalk, gated_page, walked_rows},
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
///
/// A term one Entity in ten holds fills a page of a hundred within it and
/// skips the index read a common term otherwise pays before its walk: the
/// candidates of an equality 100 000 of 500 000 Entities hold come from a
/// bitmap of every one of them, 15 to 20 ms for the bound's 10 000. A walk
/// that falls short costs about 1.7 ms, one probe of an equality being about
/// 1.7 µs.
pub const SHORT_WALK: u32 = 1000;

/// The most candidates an index read hands to the page, however the index
/// yields them.
///
/// The bound trades two placements of the matches, which no count tells apart
/// before the read. Matches holding the last identifiers of the selection
/// make the walk probe nearly every Entity: among 500 000 Entities that costs
/// 0.3 to 0.9 s, where the index answers 10 000 such matches in 8 to 22 ms.
/// Matches spread through the selection fill the page soon into the walk, so
/// a term most Entities hold walks on every page, after reading the bound's
/// candidates: 10 000 of them cost about 6 ms from a value index, whether an
/// equality or a range reads it, and 28 ms as carriers of an attribute name,
/// where 50 000 cost 30 to 42 ms. A pattern whose trigrams most values share
/// gathers every posting first, 70 to 150 ms. A bound derived from the
/// selection's size serves one placement only at the other's cost.
pub const CANDIDATE_BOUND: u64 = 10_000;

/// The most candidates the index path hands to the page, by how they are
/// read: [`CANDIDATE_BOUND`] for every read but a hop's.
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
        CandidateRead::Rechecked | CandidateRead::Ranged | CandidateRead::Probed => CandidateBound::AtMost(CANDIDATE_BOUND),
        CandidateRead::Linked => CandidateBound::Whole,
    }
}

/// The projected page of `source` for a `q` with keyed terms, read in the
/// order `keyed` lists them, and the order its rows come back in, its rows
/// carrying what `columns` asks for.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn keyed_page(
    source: &PageSource<'_>,
    q: &CompiledFilter,
    keyed: Vec<KeyedCandidates<'_>>,
    columns: GatedColumns,
) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let walked = walked_rows(source.rows);
    let walk = PageWalk {
        source: *source,
        walked: source.condition(QFilter::Omitted, GeoFilter::Matched)?,
        probe: compile(q.reading(), &InstanceScan::Current(&walked), source.names),
        short: SHORT_WALK,
    };
    let reads = keyed
        .into_iter()
        .map(|keyed| {
            Ok(IndexRead {
                candidates: keyed.select,
                bound: page_bound(keyed.read),
                narrowed: source.condition(QFilter::Except(keyed.term), GeoFilter::Matched)?,
            })
        })
        .collect::<Result<Vec<IndexRead>, RenderError>>()?;
    gated_page(source, walk, reads, columns)
}

#[cfg(test)]
mod tests {
    use crate::{entity::build_query, entity_total::EntityTotal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::{EntitySelection, EntitySelector},
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse},
        page_window::PageWindow,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_postgresql_query::{attribute_carriers::AttributeCarriers, query_names::QueryNames};
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("temperature", 12), ("source", 13), ("hasTrip", 14), ("status", 15)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::from_pairs([("https://example.com/Sensor".parse().expect("valid type name"), EntityTypeNameKey::from(3))]),
            ..QueryNames::default()
        }
    }

    fn query(q: &str) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.q = Some(CompiledFilter::new(parse(q).expect("valid q"), LanguageMatching::TagEquality));
        query
    }

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    /// `q=status=="fail"` beside `attrs=temperature`, of `selected` types.
    fn failing_beside_attrs(selected: &[&str]) -> CompiledEntityQuery {
        let mut query = query("\"https://example.com/status\"==\"fail\"");
        query.selection.selectors = selected
            .iter()
            .map(|ty| EntitySelector::builder().type_expr(Some(TypeExpr::Type(iri(ty)))).build())
            .collect();
        query.selection.attrs = vec![iri("temperature")];
        query
    }

    /// The census counts 40 instances of `temperature` and 100 000 of
    /// `source`: the pattern's candidates fit the bound of 10 000, so it is
    /// keyed ahead of the canonical first conjunct, the existence of `source`,
    /// whose carriers pass it.
    #[test]
    fn a_census_count_within_its_bound_keys_a_later_conjunct() {
        let mut names = names();
        let read = [AttributeNameKey::from(12), AttributeNameKey::from(13)];
        names.carriers = AttributeCarriers::new(&read, [(AttributeNameKey::from(12), 40), (AttributeNameKey::from(13), 100_000)]);
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/source\";\"https://example.com/temperature\"~=\"text-71.*\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names,
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-census-count-within-its-bound-keys-a-later-conjunct", built.select);
    }

    /// Of a conjunction, each keyed conjunct is read in turn: the second
    /// read's branch opens only when the first read passed the bound of
    /// 10 000 and the second stayed within it, and the full walk only when
    /// both passed it.
    #[test]
    fn a_conjunction_tries_each_keyed_conjunct_in_turn() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\"==\"x\";\"https://example.com/source\">1"),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-conjunction-tries-each-keyed-conjunct-in-turn", built.select);
    }

    /// A hop's candidates are read whole, so its branch opens whenever the
    /// reads before it passed their bounds, and the page has no full walk.
    #[test]
    fn a_hop_read_after_another_leaves_the_page_no_full_walk() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/source\">1;\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\""),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-hop-read-after-another-leaves-the-page-no-full-walk", built.select);
    }

    /// The full walk reads the selection in page order as a subquery behind
    /// `OFFSET 0` and probes each Entity outside it: from the cursor, or from
    /// the start with the offset cut outside.
    #[test]
    fn the_full_walk_walks_the_selection_in_page_order() {
        let q = query("\"https://example.com/temperature\"==\"x\"");
        for (index, window) in [
            PageWindow::offset(20, 40),
            PageWindow::after(20, "urn:ngsi-ld:Building:001".parse().expect("valid URI")),
        ]
        .into_iter()
        .enumerate()
        {
            let built = build_query(&ContextTarget::Live, &q, &window, CountPolicy::Omit, &names()).expect("valid entity query");
            assert_statement_snapshot!(
                format!("keyed-page-the-full-walk-walks-the-selection-in-page-order-{}", index + 1),
                built.select
            );
        }
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

    #[test]
    fn a_date_time_ordering_reads_its_candidates_from_a_range_of_its_index() {
        let built = build_query(
            &ContextTarget::Live,
            &query("\"https://example.com/temperature\">2024-01-15T12:00:00Z"),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_statement_snapshot!("keyed-page-a-date-time-ordering-reads-its-candidates-from-a-range-of-its-index", built.select);
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
            &query("\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\";\"https://example.com/temperature\"!=1"),
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

    /// The index branch looks each candidate up by key, so it keeps the plain
    /// join of the search row.
    #[test]
    fn a_keyed_page_beside_attrs_walks_in_identifier_order() {
        let built = build_query(
            &ContextTarget::Live,
            &failing_beside_attrs(&["Sensor"]),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_statement_snapshot!("keyed-page-a-keyed-page-beside-attrs-walks-in-identifier-order", built.select);
    }

    #[test]
    fn a_keyed_page_without_a_type_beside_attrs_walks_the_identifier_index() {
        let built = build_query(
            &ContextTarget::Live,
            &failing_beside_attrs(&[]),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_eq!(built.planning, StatementPlanning::GatedBranches);
        assert_statement_snapshot!("keyed-page-a-keyed-page-without-a-type-beside-attrs-walks-the-identifier-index", built.select);
    }

    #[test]
    fn a_page_without_a_keyed_term_takes_any_cached_plan() {
        for q in [
            "\"https://example.com/temperature\".\"https://example.com/source\"~=\"^sensor-4[0-9]$\"",
            "\"https://example.com/temperature\"!=1",
        ] {
            let built = build_query(&ContextTarget::Live, &query(q), &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            assert_eq!(built.planning, StatementPlanning::Cached, "{q}");
        }
    }
}
