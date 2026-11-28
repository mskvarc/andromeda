//! The page of an entity query whose `q` holds conjuncts an index answers:
//! an equality with one operand on an attribute's own value or on a
//! sub-attribute, a `~=` on an attribute's own value, an ordering or a range
//! on an attribute's own number, `DateTime`, `Date` or `Time` value,
//! the bare existence of an attribute, or a Linked Entity hop.
//!
//! The planner cannot price any of these terms (it underestimates a broad one
//! by orders of magnitude), so the page is a gated page:
//!
//! 1. for each keyed conjunct a B-tree yields, an equality on an attribute's
//!    own value or an ordering, the page counts the index entries bounding
//!    its candidates, up to [`SHORT_WALK`] + 1 of them, through an index-only
//!    scan of each typed field's B-tree. When they number at most
//!    [`SHORT_WALK`], the candidates are read and the page is cut from them,
//!    under the rest of `q`, so a term that rare never pays a walk. Among
//!    719 663 stop times the 15 of one trip cut their page in 0.17 ms, where
//!    walking the selection first costs 2.9 to 5.7 ms. A count past the bound stops
//!    after 1 001 entries without visiting the heap, 0.2 ms for an equality
//!    100 000 of 500 000 Sensors hold, whose candidates PostgreSQL reads
//!    through a bitmap of every one of them in 10 ms, estimating 12 800;
//! 2. the short walk probes the first [`SHORT_WALK`] Entities of the
//!    selection in page order for the whole `q`, its first keyed conjunct
//!    first;
//! 3. the continued walk probes the next [`CONTINUED_WALK`] the same way,
//!    when the short walk's matches project the page full within them;
//! 4. the index reads each keyed conjunct in turn
//!    ([`keyed_candidates`](andromeda_postgresql_query::q_predicate::keyed_term::keyed_candidates)):
//!    at most [`CANDIDATE_BOUND`] + 1 rows through the value and containment
//!    indexes, `idx_ca_sub_eq_keys` or the trigram indexes, each rechecked,
//!    from a range of a typed value index, or as carriers of an attribute
//!    name, each probed for a standing instance, or every Entity naming a
//!    linked entity the hop's inner term keeps. The page is cut from the
//!    selection narrowed to the first candidates within their bound, under
//!    the rest of `q`;
//! 5. the full walk probes each Entity of the selection in page order for the
//!    whole `q`, when every read found more candidates than its bound. A hop's
//!    candidates are read whole, so no read follows it and the page has no
//!    full walk.
//!
//! A pattern, a sub-attribute equality and an existence are read only after
//! the walks, because a GIN gathers every posting before it yields the first
//! entry and a bound saves nothing of a common term's read, which a pattern
//! whose trigrams most values share pays at 70 to 150 ms. Neither is a hop,
//! whose read joins the linked entities to the instances naming them, since
//! under a bound the planner may drive it from the instances and probe each
//! outward. An equality's containment keys are a GIN too, counted after its
//! typed fields, so an operand few typed values and most structured values
//! hold pays their postings before the walk.
//!
//! A geo-query beside the keyed terms is read the way the geo page reads it:
//! its matches first, at most 10 001 of them, and the walks probe it on each
//! Entity they yield. Each keyed read's branch tests it
//! on each candidate by its key. Tested as the set of every match, it is
//! hashed whole for the walks and read whole by an index branch however few
//! its candidates, and the short walk visits every Entity before it fills its
//! page when the matches are few: among 500 000 Entities `status=="fail"` (100 000 holders) beside a
//! geo-query 111 meet takes 1 050 ms that way and 3.3 ms read this way, and
//! `temperature>39.99` (47) beside one all of them meet 974 ms against
//! 5.8 ms. The geo read comes first because it costs the least when it
//! passes its bound, 4.5 to 20 ms, where a keyed read costs 6 to 28 ms and a
//! broad pattern 70 to 150 ms: the slowest of these pages takes 40 ms read
//! that way and 54 ms read the other way.
//!
//! The filters the walks test on each Entity they yield are read next, before
//! the keyed terms ([`filter_reads`](crate::filter_reads)): the carriers of an
//! `attrs` the census bounds, which answer, else the matches of a `scopeQ`.
//! A common term beside a scope no Entity holds then answers in 1.4 ms, where
//! the walk visits every Entity in 633 ms.

use crate::{
    entity::{GeoFilter, QFilter},
    entity_page::ProjectedPage,
    filter_reads::filter_read_before,
    gated_page::{GatedColumns, GatedReads, IndexRead, LeadingRead, PageSource, gated_page},
    geo_page::{GeoMatches, GeoWalk, geo_probe, geo_read},
    page_walk::{PageWalk, walked_rows},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    filter::compiled_filter::CompiledFilter,
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    error::RenderError,
    one_time_gate::CandidateBound,
    q_predicate::{
        instance_scan::InstanceScan,
        keyed_term::{CandidateRead, CensusCount, KeyedCandidates},
        projection::{compile, compile_led_by},
    },
};
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use sea_query::Condition;

/// How many Entities the short walk probes, and how many index entries a
/// leading read counts before the page walks.
///
/// A term one Entity in ten holds fills a page of a hundred within it and
/// skips the bounded read a common term otherwise pays before its walk. The
/// candidates of an equality 100 000 of 500 000 Entities hold come from a
/// bitmap of every one of them, 15 to 20 ms for the bound's 10 000. A walk
/// that falls short costs 1.7 to 8.5 ms, a probe costing 1.7 µs for an
/// equality and 8.5 µs for three terms. A leading read whose entries stay
/// within the length probes no more candidates than the walk probes
/// Entities, and answers.
pub const SHORT_WALK: u32 = 1000;

/// How many Entities past the short walk the continued walk probes, when the
/// short walk's matches project the page full within them.
///
/// The short walk measures how densely the matches lie where the selection
/// starts. Among 80 000 Vehicles `speed>107;status=="moving"` keeps 34 of
/// the 1 000 it visits, and the next 2 247 fill a page of a hundred, so the
/// page takes 4.4 ms, where cutting it from the 8 672 `speed` candidates, each
/// probed for `status`, takes 26 ms. A page of a hundred walks on from 21
/// matches in the short walk, one in 48 Entities, the rate that fills it
/// within both walks, and when the matches thin past the short walk the walk
/// on probes fewer Entities than the bound's 10 000 candidates.
pub const CONTINUED_WALK: u32 = 4000;

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

/// What a keyed page reads: its `q`, the terms an index answers, and the
/// matches of a geo-query beside them.
pub(crate) struct KeyedReads<'q> {
    /// The query's `q`.
    pub(crate) q: &'q CompiledFilter,
    /// The terms of `q` an index answers, in the order the page reads them.
    pub(crate) terms: Vec<KeyedCandidates<'q>>,
    /// The matches of the query's geo-query, read before the terms; `None`
    /// when the query has no geo-query with matches to read.
    pub(crate) geo: Option<GeoMatches<'q>>,
}

impl KeyedReads<'_> {
    /// How PostgreSQL may plan the page these reads cut from `query`.
    ///
    /// The page keeps one generic plan per connection when every term it
    /// reads is an equality or an ordering on an attribute's own value, `q`
    /// crosses no Linked Entity, and `query` has neither a geo-query nor a
    /// `scopeQ`. Every read then takes one access path whatever the request
    /// binds: the selection's types and every limit are literals, the walks
    /// probe each Entity by its key behind fences the planner cannot flatten,
    /// and each count and read of a term goes through the B-tree over the
    /// columns it compares. Planned with its values, the page of one trip
    /// among 719 663 stop times spends 1.5 ms planning on every request and
    /// executes on the same plan in 0.3 ms.
    ///
    /// Any other keyed page is planned with its bound values, which change
    /// what it reads. Only a literal pattern is read as a B-tree range over its
    /// anchored prefix or through the trigrams it holds, where a parameter
    /// reads every value of the attribute: among the trips
    /// `direction==0;headSign~="^Vaj.*"` takes 10 ms planned with its values
    /// and 21 ms without them. Only a literal reference geometry is made valid
    /// once rather than on every Entity a walk probes, and only a literal
    /// `scopeQ` lets a probe reject an Entity through the scope index. Without
    /// the operand of a hop's inner term, the planner reads that term's index
    /// on every probe instead of following the walked Entity's Relationship,
    /// so `hasTrip{headSign}=="Vajnory"` takes 88 ms where 19 ms suffice. A
    /// sub-attribute equality and an existence, read through a GIN as a
    /// pattern is, are planned with their values as well.
    pub(crate) fn planning(&self, query: &CompiledEntityQuery) -> StatementPlanning {
        let selection = &query.selection;
        let b_tree_reads = self.terms.iter().all(|keyed| keyed.entries.is_some());
        if b_tree_reads && selection.geo.is_none() && selection.scope_q.is_none() && !self.q.filter().contains_linked_hop() {
            StatementPlanning::GenericGatedBranches
        } else {
            StatementPlanning::GatedBranches
        }
    }
}

/// The projected page of `source` for a `q` with keyed terms, read in the
/// order `reads` lists them after the geo matches it names, and the order its
/// rows come back in, its rows carrying what `columns` asks for.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn keyed_page(source: &PageSource<'_>, reads: KeyedReads<'_>, columns: GatedColumns) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let KeyedReads { q, terms, geo } = reads;
    let census = terms.first().map_or(CensusCount::Unbounded, |keyed| keyed.census);
    let probe = walk_probe(source, q, &terms);
    match geo {
        None => {
            let walk = PageWalk {
                source: *source,
                walked: source.conditions(QFilter::Omitted, GeoFilter::Matched)?,
                probe,
                short: SHORT_WALK,
                continued: walk_on(columns),
            };
            let reads = GatedReads {
                leading: leading_reads(source, &terms, GeoFilter::Matched)?,
                trailing: filter_read_before(source, GeoFilter::Matched, term_reads(source, terms, GeoFilter::Matched)?, census)?,
            };
            gated_page(source, walk, reads, columns)
        }
        Some(matches) => {
            let geo_walk = GeoWalk::of(source);
            let walked_source = geo_walk.source(source);
            let walk = PageWalk {
                source: walked_source,
                walked: walked_source.conditions(QFilter::Omitted, GeoFilter::Omitted)?,
                probe: geo_probe(source, matches.geo, Some(probe))?,
                short: SHORT_WALK,
                continued: walk_on(columns),
            };
            let reads = GatedReads {
                leading: leading_reads(source, &terms, GeoFilter::Probed)?,
                trailing: [geo_read(source, matches)?]
                    .into_iter()
                    .chain(filter_read_before(
                        source,
                        GeoFilter::Probed,
                        term_reads(source, terms, GeoFilter::Probed)?,
                        census,
                    )?)
                    .collect(),
            };
            gated_page(source, walk, reads, columns)
        }
    }
}

/// How many Entities past the short walk a page whose rows carry `columns`
/// walks on: none when they carry the total. Its index branches count the
/// total as they read their candidates, and a page a walk answers leaves the
/// total to a count of its own, which reads them again. The counted page of
/// `speed>107;status=="moving"` among 80 000 Vehicles takes 40 ms walked on
/// and 35 ms cut from its candidates.
const fn walk_on(columns: GatedColumns) -> Option<u32> {
    match columns.total {
        CountPolicy::Emit => None,
        CountPolicy::Omit => Some(CONTINUED_WALK),
    }
}

/// `q` as the walks probe each Entity they visit, over [`walked_rows`], its
/// first keyed conjunct tested first, which rejects on one probe each Entity
/// it does not hold.
fn walk_probe(source: &PageSource<'_>, q: &CompiledFilter, terms: &[KeyedCandidates<'_>]) -> Condition {
    let walked = walked_rows(source.rows);
    let scan = InstanceScan::Walked(&walked);
    match terms.first() {
        Some(first) => compile_led_by(q.reading(), first.term, &scan, source.names),
        None => compile(q.reading(), &scan, source.names),
    }
}

/// The reads the page tries before its walks, of the `terms` whose index
/// entries a B-tree counts, so a count bounded by the short walk's length
/// stops after as many entries. Each branch is filtered by every filter but
/// its term, the geo-query as `geo` says.
fn leading_reads(source: &PageSource<'_>, terms: &[KeyedCandidates<'_>], geo: GeoFilter) -> Result<Vec<LeadingRead>, RenderError> {
    terms
        .iter()
        .filter_map(|keyed| {
            keyed.entries.clone().map(|entries| {
                Ok(LeadingRead {
                    entries,
                    // clone: the trailing read of the same term takes the candidates under its own bound
                    candidates: keyed.select.clone(),
                    narrowed: source.condition(QFilter::Except(keyed.term), geo)?,
                })
            })
        })
        .collect()
}

/// The read of each of `terms`, its branch filtered by every filter but the
/// term, the geo-query as `geo` says.
fn term_reads(source: &PageSource<'_>, terms: Vec<KeyedCandidates<'_>>, geo: GeoFilter) -> Result<Vec<IndexRead>, RenderError> {
    terms
        .into_iter()
        .map(|keyed| {
            Ok(IndexRead {
                candidates: keyed.select,
                bound: page_bound(keyed.read),
                narrowed: source.condition(QFilter::Except(keyed.term), geo)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::build_query,
        entity_total::EntityTotal,
        geo_page_fixtures::{built, filtered, first_page, past_first_sensor, sensors_within_square},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_constraint::EntityConstraint,
        entity_selection::EntitySelection,
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching},
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
        page_window::PageWindow,
        scope::scope_query::ScopeQuery,
        type_selection::TypeSelection,
    };
    use andromeda_postgresql_query::{attribute_carriers::AttributeCarriers, query_names::QueryNames};
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [
                    ("temperature", 12),
                    ("source", 13),
                    ("hasTrip", 14),
                    ("status", 15),
                    ("speed", 16),
                    ("hasStop", 17),
                    ("departureTime", 18),
                ]
                .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::from_pairs([("https://example.com/Sensor".parse().expect("valid type name"), EntityTypeNameKey::from(3))]),
            ..QueryNames::default()
        }
    }

    fn query(q: &str) -> CompiledEntityQuery {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.q = Some(CompiledFilter::new(
            EXAMPLE_NAMESPACE
                .expanded(q.parse::<Filter<Written>>().expect("valid q"))
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        ));
        query
    }

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    /// `q` over the Sensors.
    fn sensors(q: &str) -> CompiledEntityQuery {
        let mut query = query(q);
        query.selection.selectors = vec![
            EntityConstraint::builder()
                .type_selection(Some(TypeSelection::single(iri("Sensor").into())))
                .build(),
        ];
        query
    }

    /// The first page of a hundred, read with the one Entity past it that
    /// tells whether a next page exists.
    fn first_hundred() -> PageWindow {
        PageWindow::offset(101, 0)
    }

    /// The leading read counts the trip's index entries up to one past the
    /// short walk's length and, when they fit, cuts the page from its
    /// candidates under no gate a walk sets, so a rare term pays no walk. Past
    /// that length the short walk answers a common term, then the continued
    /// walk and the bounded read.
    #[test]
    fn a_rare_term_is_cut_from_its_leading_read_before_any_walk() {
        let built = build_query(
            &ContextTarget::Live,
            &sensors("\"https://example.com/hasTrip\"==urn:ngsi-ld:GtfsTrip:36010_01_17_18983"),
            &first_hundred(),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-rare-term-is-cut-from-its-leading-read-before-any-walk", built.select);
    }

    /// Both conjuncts are read through a B-tree, so both lead. A short walk
    /// keeping 21 of the 101 matches the page needs projects it full within
    /// the next 4 000 Entities, which the continued walk visits past the
    /// first 1 000, needing 80 matches at most.
    #[test]
    fn a_short_walk_that_falls_short_walks_on_past_the_entities_it_visited() {
        let built = build_query(
            &ContextTarget::Live,
            &sensors("\"https://example.com/speed\">107;\"https://example.com/status\"==\"moving\""),
            &first_hundred(),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-short-walk-that-falls-short-walks-on-past-the-entities-it-visited", built.select);
    }

    /// A String ordering has no index, so the stop leads alone, and the walks
    /// probe it before the departure times.
    #[test]
    fn a_stop_window_reads_the_stop_first_and_probes_it_first() {
        let built = build_query(
            &ContextTarget::Live,
            &sensors(
                "\"https://example.com/hasStop\"==urn:ngsi-ld:GtfsStop:000000055300004;\"https://example.com/departureTime\">=\"07:00:00\";\"https://example.com/departureTime\"<\"08:00:00\"",
            ),
            &first_hundred(),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        assert_statement_snapshot!("keyed-page-a-stop-window-reads-the-stop-first-and-probes-it-first", built.select);
    }

    /// A page needing more matches than the short walk visits Entities
    /// cannot fill there, so its short walk visits none and nothing walks on,
    /// while its leading read still answers a rare term.
    #[test]
    fn a_page_deeper_than_the_short_walk_still_reads_its_leading_candidates() {
        let built = build_query(
            &ContextTarget::Live,
            &sensors("\"https://example.com/status\"==\"moving\""),
            &PageWindow::offset(101, 2000),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query");
        let sql = built.select.to_string(PostgresQueryBuilder);
        assert!(!sql.contains("\"continued_walk\""), "{sql}");
        assert_statement_snapshot!("keyed-page-a-page-deeper-than-the-short-walk-still-reads-its-leading-candidates", built.select);
    }

    /// Clause 6.3.13: a first page whose rows carry the total counts it on
    /// its index branches, so it never walks on; past a cursor the total is a
    /// count of its own, and the page walks on as an uncounted one does.
    #[test]
    fn a_page_whose_rows_carry_the_total_never_walks_on() {
        let q = sensors("\"https://example.com/speed\">107;\"https://example.com/status\"==\"moving\"");
        let walks_on = |window: &PageWindow, count: CountPolicy| {
            build_query(&ContextTarget::Live, &q, window, count, &names())
                .expect("valid entity query")
                .select
                .to_string(PostgresQueryBuilder)
                .contains("\"continued_walk\"")
        };
        let past_a_cursor = PageWindow::after(101, "urn:ngsi-ld:Sensor:001".parse().expect("valid URI"));
        assert!(!walks_on(&first_hundred(), CountPolicy::Emit));
        assert!(walks_on(&first_hundred(), CountPolicy::Omit));
        assert!(walks_on(&past_a_cursor, CountPolicy::Emit));
    }

    /// A pattern, a sub-attribute equality and an existence are read through
    /// a GIN and a hop through a join, so none leads.
    #[test]
    fn only_a_term_a_b_tree_yields_in_order_leads() {
        for q in [
            "\"https://example.com/status\"~=\"mov.*\"",
            "\"https://example.com/temperature\".\"https://example.com/source\"==\"sensor-43\"",
            "\"https://example.com/temperature\"",
            "\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\"",
        ] {
            let sql = build_query(&ContextTarget::Live, &sensors(q), &first_hundred(), CountPolicy::Omit, &names())
                .expect("valid entity query")
                .select
                .to_string(PostgresQueryBuilder);
            assert!(!sql.contains("LIMIT 1001"), "{q}: {sql}");
        }
    }

    /// `q=status=="fail"` beside `attrs=temperature`, of `selected` types.
    fn failing_beside_attrs(selected: &[&str]) -> CompiledEntityQuery {
        let mut query = query("\"https://example.com/status\"==\"fail\"");
        query.selection.selectors = selected
            .iter()
            .map(|ty| EntityConstraint::builder().type_selection(Some(TypeSelection::single(iri(ty).into()))).build())
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
        assert_eq!(built.planning, StatementPlanning::GenericGatedBranches);
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

    /// Each read of an equality or an ordering on an own value takes one
    /// access path whatever is bound, as do the walks beside it and a
    /// disjunction they probe, so the page keeps one generic plan per
    /// connection, and its gated-off branches launch no parallel worker.
    #[test]
    fn an_own_value_equality_or_ordering_page_keeps_one_generic_plan_per_connection() {
        for q in [
            "\"https://example.com/hasTrip\"==\"urn:ngsi-ld:GtfsTrip:1\"",
            "\"https://example.com/speed\">107;\"https://example.com/status\"==\"moving\"",
            "\"https://example.com/hasStop\"==\"urn:ngsi-ld:GtfsStop:1\";\"https://example.com/departureTime\">=\"07:00:00\";\"https://example.com/departureTime\"<\"08:00:00\"",
            "\"https://example.com/status\"==\"fail\";(\"https://example.com/speed\"==1|\"https://example.com/speed\"==2)",
        ] {
            let built = build_query(&ContextTarget::Live, &query(q), &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            assert_eq!(built.planning, StatementPlanning::GenericGatedBranches, "{q}");
        }
    }

    /// A pattern, a sub-attribute equality, an existence and a hop, read or
    /// only probed, take another access path without their values, as does a
    /// geo-query or a `scopeQ` beside an equality, so each such page is
    /// planned with its bound values, and runs serially.
    #[test]
    fn a_keyed_page_whose_values_change_its_reads_is_planned_with_them() {
        let mut scoped = query("\"https://example.com/status\"==\"fail\"");
        scoped.selection.scope_q = Some("/Madrid/#".parse::<ScopeQuery>().expect("valid scope query"));
        let queries = [
            "\"https://example.com/temperature\"~=\"^text-71.*\"",
            "\"https://example.com/temperature\".\"https://example.com/source\"==\"uncalibrated\"",
            "\"https://example.com/temperature\"",
            "\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\"",
            "\"https://example.com/status\"==\"fail\";(\"https://example.com/speed\"==1|\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\")",
            "\"https://example.com/status\"==\"fail\";\"https://example.com/temperature\"~=\"^text-71.*\"",
        ]
        .map(query)
        .into_iter()
        .chain([scoped]);
        for query in queries {
            let built = build_query(&ContextTarget::Live, &query, &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
            assert_eq!(
                built.planning,
                StatementPlanning::GatedBranches,
                "{:?}",
                query.selection.q.as_ref().map(CompiledFilter::filter)
            );
        }
        let beside_geo = built(
            &filtered(sensors_within_square(), "\"https://example.com/status\"==\"fail\""),
            &first_page(),
            CountPolicy::Omit,
        );
        assert_eq!(beside_geo.planning, StatementPlanning::GatedBranches);
    }

    /// The index branch looks each candidate up by key, so it keeps the plain
    /// join of the search row and its filters.
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
        assert_eq!(built.planning, StatementPlanning::GenericGatedBranches);
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
        assert_eq!(built.planning, StatementPlanning::GenericGatedBranches);
        assert_statement_snapshot!("keyed-page-a-keyed-page-without-a-type-beside-attrs-walks-the-identifier-index", built.select);
    }

    /// Every walk tests `attrs` on each Entity it visits behind a fence of its
    /// own, which the planner cannot turn into a semi-join over every `attrs`
    /// row; a walk with nothing beside its selectors has no fence, and a walk
    /// beside a geo-query probes it instead.
    #[test]
    fn a_walk_tests_its_filters_on_each_entity_it_visits() {
        let fences = |query: &CompiledEntityQuery| {
            build_query(&ContextTarget::Live, query, &PageWindow::offset(100, 0), CountPolicy::Omit, &names())
                .expect("valid entity query")
                .select
                .to_string(PostgresQueryBuilder)
                .matches(r#"AS "filtered" ON TRUE"#)
                .count()
        };
        assert_eq!(fences(&failing_beside_attrs(&["Sensor"])), 3);
        let mut typed = failing_beside_attrs(&["Sensor"]);
        typed.selection.attrs = Vec::new();
        assert_eq!(fences(&typed), 0);
        let geo_fences = |query: &CompiledEntityQuery| {
            built(query, &first_page(), CountPolicy::Omit)
                .select
                .to_string(PostgresQueryBuilder)
                .matches(r#"AS "filtered" ON TRUE"#)
                .count()
        };
        assert_eq!(geo_fences(&failing_within_square()), 0);
        let mut beside_attrs = failing_within_square();
        beside_attrs.selection.attrs = vec![iri("temperature")];
        assert_eq!(geo_fences(&beside_attrs), 3);
    }

    /// The walks test their filters on each Entity they yield, through its
    /// search row and after the walk's `LIMIT`, so the short walk visits its
    /// length of the selection whatever the filters keep.
    #[test]
    fn a_short_walk_visits_its_length_of_the_selection_whatever_its_filters_keep() {
        let sql = build_query(
            &ContextTarget::Live,
            &failing_beside_attrs(&["Sensor"]),
            &PageWindow::offset(100, 0),
            CountPolicy::Omit,
            &names(),
        )
        .expect("valid entity query")
        .select
        .to_string(PostgresQueryBuilder);
        let tested_after_the_walk =
            r#"AS "walked" INNER JOIN LATERAL (SELECT 1 FROM "entity_search" WHERE "entity_search"."entity_key" = "walked"."entity_key""#;
        assert_eq!(sql.matches(tested_after_the_walk).count(), 3);
    }

    /// `scopeQ` reads the scope paths of the search row each walked Entity is
    /// looked up by.
    #[test]
    fn scope_q_beside_a_keyed_term_is_tested_on_each_walked_entity() {
        let mut query = failing_beside_attrs(&["Sensor"]);
        query.selection.attrs = Vec::new();
        query.selection.scope_q = Some("/Madrid/#".parse::<ScopeQuery>().expect("valid scope query"));
        let built = build_query(&ContextTarget::Live, &query, &PageWindow::offset(100, 0), CountPolicy::Omit, &names()).expect("valid entity query");
        assert_statement_snapshot!("keyed-page-scope-q-beside-a-keyed-term-is-tested-on-each-walked-entity", built.select);
    }

    /// `q=status=="fail"` beside the shared geo-query.
    fn failing_within_square() -> CompiledEntityQuery {
        filtered(sensors_within_square(), "\"https://example.com/status\"==\"fail\"")
    }

    /// The walks probe the geo-query on each Member they visit, the geo
    /// matches are read first and their branch filtered by `q`, and the keyed
    /// branch tests the geo-query on each candidate, so no branch reads the
    /// set of every match.
    #[test]
    fn a_keyed_page_beside_a_geo_query_reads_the_geo_matches_first() {
        let select = built(&failing_within_square(), &first_page(), CountPolicy::Omit).select;
        assert!(!select.to_string(PostgresQueryBuilder).contains("SELECT DISTINCT"));
        assert_statement_snapshot!("keyed-page-a-keyed-page-beside-a-geo-query-reads-the-geo-matches-first", select);
    }

    #[test]
    fn a_keyed_page_beside_a_geo_query_without_a_type_walks_every_entity() {
        let mut query = failing_within_square();
        query.selection.selectors = Vec::new();
        assert_statement_snapshot!(
            "keyed-page-a-keyed-page-beside-a-geo-query-without-a-type-walks-every-entity",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    #[test]
    fn attrs_beside_a_keyed_term_and_a_geo_query_are_tested_on_each_walked_member() {
        let mut query = failing_within_square();
        query.selection.attrs = vec![iri("temperature")];
        assert_statement_snapshot!(
            "keyed-page-attrs-beside-a-keyed-term-and-a-geo-query-are-tested-on-each-walked-member",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    /// A hop's candidates are read whole, so the geo matches are read before
    /// it and the page has no full walk.
    #[test]
    fn a_hop_beside_a_geo_query_reads_the_geo_matches_before_the_hop() {
        let query = filtered(
            sensors_within_square(),
            "\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\"",
        );
        assert_statement_snapshot!(
            "keyed-page-a-hop-beside-a-geo-query-reads-the-geo-matches-before-the-hop",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    /// Clause 6.3.13: the geo branch reads every match within its bound before
    /// it cuts the window, as the keyed branches read their candidates, so
    /// the leading keyed read, the geo read and the trailing keyed read each
    /// carry the total.
    #[test]
    fn a_counted_keyed_page_beside_a_geo_query_carries_its_total_on_every_index_branch() {
        let page = built(&failing_within_square(), &first_page(), CountPolicy::Emit);
        assert!(matches!(page.total, EntityTotal::OnPageRows { .. }), "a first page carries the total it can");
        assert_eq!(page.select.to_string(PostgresQueryBuilder).matches("COUNT(*) OVER ()").count(), 3);
        assert_statement_snapshot!(
            "keyed-page-a-counted-keyed-page-beside-a-geo-query-carries-its-total-on-every-index-branch",
            page.select
        );
    }

    #[test]
    fn a_keyed_page_beside_a_geo_query_past_a_cursor_starts_every_branch_past_it() {
        assert_statement_snapshot!(
            "keyed-page-a-keyed-page-beside-a-geo-query-past-a-cursor-starts-every-branch-past-it",
            built(&failing_within_square(), &past_first_sensor(), CountPolicy::Omit).select
        );
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
