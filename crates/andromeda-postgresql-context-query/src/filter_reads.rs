//! The reads a gated page takes of the filters its walks test on each Entity
//! they yield: the carriers of `attrs` (clause 5.7.2.4) and the matches of
//! `scopeQ` (clause 4.19).
//!
//! A walk tests these filters on each Entity after its `LIMIT`, so when no
//! other read answers, a filter few Entities pass makes the full walk visit
//! the whole selection. Among 500 000 scoped Entities a common keyed term
//! beside a `scopeQ` no Entity matches takes 633 ms that way, the pattern
//! tested on each Entity at about a microsecond, and 1.4 ms read from the
//! scope index. Each filter is therefore one more read of the page, its
//! candidates a superset the read's branch rechecks under the whole filter.
//!
//! The carriers of `attrs` are read only when the census counts at most
//! [`CANDIDATE_BOUND`] instances of the Attributes `attrs` names. An Entity
//! carries an Attribute through an instance of it, so the read then holds
//! every carrier and answers, and only a hop's whole read follows it, so a
//! page with a hop never walks: beside a common keyed term, 50 carriers answer
//! in 1.1 ms where the walk takes 359 ms. Without the census the read would
//! also run for an Attribute too common to answer, and 25 000 carriers read
//! through the GIN cost 9 to 20 ms more than the walk that fills the page. A
//! keyed term the census fits to its bound answers the same way, so beside it
//! the page reads no filter.
//!
//! A `scopeQ` has no count to read ahead, so its matches are read, at most
//! [`SCOPE_BOUND`] + 1 of them, through the partial index over the scope
//! paths for a selective pattern and by a sequential scan for one nearly
//! every Entity holds. A filter read comes after the geo read and
//! before the keyed terms. After the geo read, a scope read costs a broad
//! geo-query 3 to 6 ms and spares a small one its matches: beside a geo-query
//! 111 Entities meet, a scope 5 000 Entities hold costs 0.8 ms read after the
//! geo matches and 16.6 ms before them. Before the keyed terms, a selective
//! scope answers without the bound's keyed candidates (1.4 ms beside a common
//! term, against 13.7 ms read after them), and the slowest page takes 52 ms
//! where reading the scope last takes 67 ms. A rare keyed term beside a scope
//! thousands of Entities hold pays the scope's branch instead, 31.9 ms
//! against 1.2 ms for 5 000 of them, as a rare term beside a geo-query pays
//! the geo branch.
//!
//! Projected members (`pick`, `omit`) have no read: the walks test them on
//! each Entity they yield.

use crate::{
    carried_names::attrs_carriers,
    entity::{GeoFilter, QFilter, scope_paths_condition},
    gated_page::{IndexRead, PageSource},
    geo_page::geo_page_query,
    keyed_page::CANDIDATE_BOUND,
};
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, entity::compiled::CompiledEntityQuery, entity_selection::EntitySelection, page_window::PageWindow};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    one_time_gate::CandidateBound,
    q_predicate::keyed_term::{CensusCount, KeyedConjuncts, keyed_conjuncts},
    query_names::QueryNames,
};
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Condition, DynIden, Query, SelectStatement};
use std::iter::once;

/// The most `scopeQ` matches a page reads before it tries its next read.
///
/// A match costs 5 to 7 µs to read, filter and sort, so the bound's matches
/// cost about 60 ms. A read that passes the bound costs 2.5 ms when nearly
/// every Entity matches, read by a sequential scan, and about 11 ms for a
/// pattern a tenth of 500 000 Entities hold. The bound trades two placements
/// of the matches, which no count tells apart before the read: 5 000 matches
/// holding the last identifiers cost 11.6 to 26.3 ms read, where a bound of
/// 1 000 leaves them to the walk at 357 to 404 ms, and 5 000 spread through
/// the selection cost 23.9 ms read where the walk fills the page in 11.4 ms.
pub const SCOPE_BOUND: u64 = 10_000;

/// The reads of a page of `source` that reads the filters its walks test
/// before `later`, whose first read the census says `later_census` of:
/// `later` alone when the census fits that read to its bound, since it
/// answers; else the carriers of `attrs` when the census bounds them within
/// [`CANDIDATE_BOUND`], since they answer, followed only by a hop's whole
/// read from `later`; else the matches of `scopeQ`, then `later`; `later`
/// alone when the query names neither. A filter read's branch keeps the whole
/// filter, the geo-query as `geo` says.
///
/// # Errors
///
/// Fails as rendering the plain page's filter fails: a reference geometry
/// with no EWKB spelling, or a query-language rule a renderer decides first.
pub(crate) fn filter_read_before(
    source: &PageSource<'_>,
    geo: GeoFilter,
    later: Vec<IndexRead>,
    later_census: CensusCount,
) -> Result<Vec<IndexRead>, RenderError> {
    match later_census {
        CensusCount::WithinBound => return Ok(later),
        CensusCount::Unbounded => {}
    }
    let selection = &source.query.selection;
    let carriers = census_of_attrs(selection, source.names)
        .filter(|instances| CARRIERS_BOUND.admits(*instances))
        .and_then(|_| search_keys(source.schema, |rows| attrs_carriers(selection, source.names, rows)));
    if let Some(candidates) = carriers {
        let read = IndexRead {
            candidates,
            bound: CARRIERS_BOUND,
            narrowed: source.condition(QFilter::Whole, geo)?,
        };
        return Ok(once(read)
            .chain(later.into_iter().filter(|later| later.bound == CandidateBound::Whole))
            .collect());
    }
    let scope = selection
        .scope_q
        .as_ref()
        .and_then(|scope_q| search_keys(source.schema, |_| Some(scope_paths_condition(scope_q))));
    let Some(candidates) = scope else {
        return Ok(later);
    };
    let read = IndexRead {
        candidates,
        bound: CandidateBound::AtMost(SCOPE_BOUND),
        narrowed: source.condition(QFilter::Whole, geo)?,
    };
    Ok(once(read).chain(later).collect())
}

/// The most carriers the `attrs` read hands the page, and the census count
/// under which it runs: [`CANDIDATE_BOUND`], as a keyed read's.
const CARRIERS_BOUND: CandidateBound = CandidateBound::AtMost(CANDIDATE_BOUND);

/// The instances the census counts of the Attributes `attrs` names, summed,
/// an Attribute the dictionary never recorded counting none; `None` when
/// `attrs` names none, or the census did not count one of them.
fn census_of_attrs(selection: &EntitySelection, names: &QueryNames) -> Option<u64> {
    if selection.attrs.is_empty() {
        return None;
    }
    selection
        .attrs
        .iter()
        .try_fold(0_u64, |sum, attr| match names.attributes.get_str(attr.as_str()) {
            Some(key) => names.carriers.instances(key).map(|instances| sum.saturating_add(instances)),
            None => Some(sum),
        })
}

/// `SELECT "entity_key" FROM "entity_search" WHERE <condition>` in the branch
/// `schema` names, `condition` rendered over that search row: the keys of the
/// search rows it keeps. `None` when `condition` gives none.
pub(crate) fn search_keys(schema: Option<&DynIden>, condition: impl FnOnce(&EntityRowRefs) -> Option<Condition>) -> Option<SelectStatement> {
    let rows = EntityRowRefs::unaliased(schema.cloned());
    let condition = condition(&rows)?;
    let mut keys = Query::select();
    keys.expr(rows.search_column(EntitySearch::EntityKey))
        .from(rows.table(EntitySearch::Table))
        .cond_where(condition);
    Some(keys.take())
}

/// The Attributes whose census counts the page of `query` over `window`, its
/// total as `count` asks, reads before it is rendered, sorted, each once: the
/// carriers the contended keyed conjuncts of its `q` are read through, and
/// the Attributes its `attrs` names that the dictionary recorded, when a
/// keyed term of `q` or a geo page cuts the page and so may read their
/// carriers.
#[must_use]
pub fn counted_attributes(query: &CompiledEntityQuery, names: &QueryNames, window: &PageWindow, count: CountPolicy) -> Vec<AttributeNameKey> {
    let conjuncts = query
        .selection
        .q
        .as_ref()
        .map_or(KeyedConjuncts::Unkeyed, |q| keyed_conjuncts(q.reading(), names));
    let (mut counted, keyed) = match conjuncts {
        KeyedConjuncts::Unkeyed => (Vec::new(), false),
        KeyedConjuncts::Single => (Vec::new(), true),
        KeyedConjuncts::Contended(carriers) => (carriers, true),
    };
    if keyed || geo_page_query(query, window, count).is_some() {
        counted.extend(query.selection.attrs.iter().filter_map(|attr| names.attributes.get_str(attr.as_str())));
    }
    counted.sort_unstable_by_key(|key| key.get());
    counted.dedup();
    counted
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{EntityQuery, build_query},
        entity_total::EntityTotal,
        filter_reads::counted_attributes,
        geo_page_fixtures::{filtered, first_page, iri, names, sensors, sensors_within_square},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        filter::value_path::ValuePath,
        ordering::{compiled_ordering_term::CompiledOrderingTerm, expanded_ordering_target::ExpandedOrderingTarget, ordering_direction::OrderingDirection},
        scope::scope_query::ScopeQuery,
    };
    use andromeda_postgresql_query::{attribute_carriers::AttributeCarriers, query_names::QueryNames};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    const STATUS_FAILS: &str = "\"https://example.com/status\"==\"fail\"";

    /// `query` filtered by the `scopeQ` `scope`.
    fn scoped(mut query: CompiledEntityQuery, scope: &str) -> CompiledEntityQuery {
        query.selection.scope_q = Some(scope.parse::<ScopeQuery>().expect("valid scope query"));
        query
    }

    /// `query` selecting the Entities that carry one of `attributes`.
    fn carrying(mut query: CompiledEntityQuery, attributes: &[&str]) -> CompiledEntityQuery {
        query.selection.attrs = attributes.iter().map(|name| iri(name)).collect();
        query
    }

    /// The shared dictionary with a census counting `counts`, each an
    /// attribute key and its instances.
    fn census(counts: &[(i32, u64)]) -> QueryNames {
        let read: Vec<AttributeNameKey> = counts.iter().map(|(key, _)| AttributeNameKey::from(*key)).collect();
        let mut names = names();
        names.carriers = AttributeCarriers::new(&read, counts.iter().map(|(key, instances)| (AttributeNameKey::from(*key), *instances)));
        names
    }

    fn page(query: &CompiledEntityQuery, names: &QueryNames, count: CountPolicy) -> EntityQuery {
        build_query(&ContextTarget::Live, query, &first_page(), count, names).expect("valid entity query")
    }

    fn sql(query: &CompiledEntityQuery, names: &QueryNames) -> String {
        page(query, names, CountPolicy::Omit).select.to_string(PostgresQueryBuilder)
    }

    /// The text of the CTE `name` of `sql`, up to the next CTE.
    fn cte<'s>(sql: &'s str, name: &str) -> &'s str {
        let header = format!("\"{name}\" AS  MATERIALIZED (");
        let start = sql.find(&header).unwrap_or_else(|| panic!("no CTE {name}")) + header.len();
        let rest = &sql[start..];
        &rest[..rest.find(" AS  MATERIALIZED (").unwrap_or(rest.len())]
    }

    /// The scope matches are read first, their branch filtered by `q` and
    /// rechecking `scopeQ`; the keyed read opens only past their bound, and
    /// the full walk past both.
    #[test]
    fn a_keyed_page_beside_a_scope_query_reads_its_matches_before_the_terms() {
        let query = scoped(filtered(sensors(), STATUS_FAILS), "/Rare/Fifty");
        let built = page(&query, &names(), CountPolicy::Omit);
        let sql = built.select.to_string(PostgresQueryBuilder);
        assert!(cte(&sql, "candidates").contains("\"scope_paths\" ~"), "{sql}");
        assert!(cte(&sql, "candidates_2").contains("\"current_attributes\""), "{sql}");
        assert_statement_snapshot!(
            "filter-reads-a-keyed-page-beside-a-scope-query-reads-its-matches-before-the-terms",
            built.select
        );
    }

    /// The census counts 50 instances of `calibration`, so its carriers are
    /// all read and answer: no keyed read follows them.
    #[test]
    fn a_keyed_page_beside_attrs_the_census_bounds_reads_their_carriers_alone() {
        let query = carrying(filtered(sensors(), STATUS_FAILS), &["calibration"]);
        let built = page(&query, &census(&[(16, 50)]), CountPolicy::Omit);
        let sql = built.select.to_string(PostgresQueryBuilder);
        assert!(cte(&sql, "candidates").contains("\"attribute_name_keys\" @>"), "{sql}");
        assert!(!sql.contains("\"candidates_2\""), "{sql}");
        assert_statement_snapshot!(
            "filter-reads-a-keyed-page-beside-attrs-the-census-bounds-reads-their-carriers-alone",
            built.select
        );
    }

    #[test]
    fn attrs_the_census_counts_past_the_bound_are_left_to_the_walks() {
        let query = carrying(filtered(sensors(), STATUS_FAILS), &["calibration"]);
        assert_eq!(sql(&query, &census(&[(16, 20_000)])), sql(&query, &names()));
    }

    #[test]
    fn attrs_without_a_census_count_are_left_to_the_walks() {
        let query = scoped(carrying(filtered(sensors(), STATUS_FAILS), &["calibration"]), "/A");
        let sql = sql(&query, &names());
        let first = cte(&sql, "candidates");
        assert!(first.contains("\"scope_paths\" ~"), "{sql}");
        assert!(!first.contains("\"attribute_name_keys\""), "{sql}");
    }

    /// `attrs` keeps an Entity carrying any of its Attributes, so the census
    /// bounds the carriers by the sum of their instances.
    #[test]
    fn the_census_bounds_attrs_by_the_sum_of_their_attributes() {
        let query = carrying(filtered(sensors(), STATUS_FAILS), &["calibration", "inspection"]);
        assert_eq!(sql(&query, &census(&[(16, 6_000), (17, 5_000)])), sql(&query, &names()));
        let read = sql(&query, &census(&[(16, 4_000), (17, 5_000)]));
        assert!(cte(&read, "candidates").contains("\"attribute_name_keys\" @>"), "{read}");
    }

    /// An Attribute the dictionary never recorded is carried by no Entity, so
    /// the read finds no candidate and answers at once.
    #[test]
    fn attrs_no_entity_carries_are_read_as_no_candidate() {
        let query = carrying(filtered(sensors(), STATUS_FAILS), &["neverRecorded"]);
        let sql = sql(&query, &names());
        assert!(cte(&sql, "candidates").contains("WHERE FALSE"), "{sql}");
        assert!(!sql.contains("\"candidates_2\""), "{sql}");
    }

    /// The geo matches are read first; the scope matches after them, their
    /// branch testing the geo-query on each candidate.
    #[test]
    fn a_geo_page_beside_a_scope_query_reads_the_geo_matches_then_the_scope_matches() {
        let query = scoped(sensors_within_square(), "/Rare/Fifty");
        let built = page(&query, &names(), CountPolicy::Omit);
        let sql = built.select.to_string(PostgresQueryBuilder);
        assert!(cte(&sql, "candidates").contains("\"location\""), "{sql}");
        assert!(cte(&sql, "candidates_2").contains("\"scope_paths\" ~"), "{sql}");
        assert_statement_snapshot!(
            "filter-reads-a-geo-page-beside-a-scope-query-reads-the-geo-matches-then-the-scope-matches",
            built.select
        );
    }

    #[test]
    fn a_geo_page_beside_attrs_the_census_bounds_reads_the_geo_matches_then_the_carriers() {
        let query = carrying(sensors_within_square(), &["calibration"]);
        let built = page(&query, &census(&[(16, 50)]), CountPolicy::Omit);
        let sql = built.select.to_string(PostgresQueryBuilder);
        assert!(cte(&sql, "candidates_2").contains("\"attribute_name_keys\" @>"), "{sql}");
        assert_statement_snapshot!(
            "filter-reads-a-geo-page-beside-attrs-the-census-bounds-reads-the-geo-matches-then-the-carriers",
            built.select
        );
    }

    #[test]
    fn a_keyed_page_beside_a_geo_query_and_a_scope_query_reads_geo_scope_then_terms() {
        let query = scoped(filtered(sensors_within_square(), STATUS_FAILS), "/Rare/Fifty");
        let built = page(&query, &names(), CountPolicy::Omit);
        let sql = built.select.to_string(PostgresQueryBuilder);
        assert!(cte(&sql, "candidates").contains("\"location\""), "{sql}");
        assert!(cte(&sql, "candidates_2").contains("\"scope_paths\" ~"), "{sql}");
        assert!(cte(&sql, "candidates_3").contains("\"current_attributes\""), "{sql}");
        assert_statement_snapshot!(
            "filter-reads-a-keyed-page-beside-a-geo-query-and-a-scope-query-reads-geo-scope-then-terms",
            built.select
        );
    }

    /// A hop's candidates are read whole, so the scope matches are read
    /// before it and the page has no full walk.
    #[test]
    fn a_hop_beside_a_scope_query_reads_the_scope_matches_before_the_hop() {
        let query = scoped(
            filtered(sensors(), "\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\""),
            "/A",
        );
        let built = page(&query, &names(), CountPolicy::Omit);
        let sql = built.select.to_string(PostgresQueryBuilder);
        assert!(cte(&sql, "candidates").contains("\"scope_paths\" ~"), "{sql}");
        assert!(!sql.contains("AS \"walk\""), "{sql}");
        assert_statement_snapshot!("filter-reads-a-hop-beside-a-scope-query-reads-the-scope-matches-before-the-hop", built.select);
    }

    /// Clause 6.3.13: the scope branch reads every match within its bound
    /// before it cuts the window, as the keyed branch reads its candidates,
    /// so both carry the total.
    #[test]
    fn a_counted_keyed_page_beside_a_scope_query_carries_its_total_on_every_index_branch() {
        let query = scoped(filtered(sensors(), STATUS_FAILS), "/Rare/Fifty");
        let built = page(&query, &names(), CountPolicy::Emit);
        assert!(matches!(built.total, EntityTotal::OnPageRows { .. }), "a first page carries the total it can");
        assert_eq!(built.select.to_string(PostgresQueryBuilder).matches("COUNT(*) OVER ()").count(), 2);
    }

    /// The census keys `counted_attributes` names for the uncounted first
    /// page of `query`.
    fn counted(query: &CompiledEntityQuery) -> Vec<i32> {
        counted_on(query, CountPolicy::Omit)
    }

    /// The census keys `counted_attributes` names for the first page of
    /// `query`, its total as `count` asks.
    fn counted_on(query: &CompiledEntityQuery, count: CountPolicy) -> Vec<i32> {
        counted_attributes(query, &names(), &first_page(), count).iter().map(|key| key.get()).collect()
    }

    #[test]
    fn the_census_counts_attrs_beside_a_keyed_term_or_on_a_geo_page() {
        assert_eq!(
            counted(&carrying(filtered(sensors(), STATUS_FAILS), &["inspection", "calibration"])),
            vec![16, 17]
        );
        assert_eq!(
            counted_on(&carrying(filtered(sensors(), STATUS_FAILS), &["calibration"]), CountPolicy::Emit),
            vec![16],
            "a keyed page is gated whatever its count"
        );
        assert_eq!(counted(&carrying(sensors_within_square(), &["calibration"])), vec![16]);
    }

    /// A geo-query whose page carries its total, or is ordered, keeps the
    /// plain page, which reads no carriers.
    #[test]
    fn the_census_counts_no_attrs_beside_a_geo_query_that_keeps_the_plain_page() {
        let beside = carrying(sensors_within_square(), &["calibration"]);
        assert_eq!(counted_on(&beside, CountPolicy::Emit), Vec::<i32>::new());
        let mut ordered = beside;
        ordered.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple("https://example.com/temperature".parse().expect("valid attribute name"))),
            direction: OrderingDirection::Asc,
            collation: None,
        }];
        assert_eq!(counted(&ordered), Vec::<i32>::new());
    }

    /// A hop's candidates are read whole and the page has no full walk, so
    /// the hop's read stays after the carriers the census bounds.
    #[test]
    fn a_hop_beside_attrs_the_census_bounds_keeps_its_read_after_the_carriers() {
        let query = carrying(
            filtered(sensors(), "\"https://example.com/hasTrip\"{\"https://example.com/temperature\"}==\"x\""),
            &["calibration"],
        );
        let sql = sql(&query, &census(&[(16, 50)]));
        assert!(cte(&sql, "candidates").contains("\"attribute_name_keys\" @>"), "{sql}");
        assert!(cte(&sql, "candidates_2").contains("\"ca_ln1\""), "{sql}");
        assert!(!sql.contains("AS \"walk\""), "{sql}");
    }

    /// A keyed term the census fits to its bound answers, so the page reads
    /// no filter beside it.
    #[test]
    fn a_keyed_term_the_census_bounds_is_read_without_the_filters() {
        let query = scoped(
            filtered(sensors(), "\"https://example.com/status\"==\"fail\";\"https://example.com/calibration\""),
            "/Rare/One",
        );
        let sql = sql(&query, &census(&[(15, 100_000), (16, 50)]));
        assert!(cte(&sql, "candidates").contains("\"attribute_name_keys\" @>"), "{sql}");
        assert!(!sql.contains("\"candidates_2\""), "{sql}");
    }

    #[test]
    fn the_census_counts_no_attrs_a_page_reads_no_carriers_for() {
        assert_eq!(
            counted(&carrying(sensors(), &["calibration"])),
            Vec::<i32>::new(),
            "the carrier page reads them uncounted"
        );
        assert_eq!(
            counted(&filtered(sensors(), STATUS_FAILS)),
            Vec::<i32>::new(),
            "one keyed term is read uncounted"
        );
        assert_eq!(
            counted(&carrying(filtered(sensors(), "\"https://example.com/status\"!=\"fail\""), &["calibration"])),
            Vec::<i32>::new(),
            "a complement keys no term"
        );
        assert_eq!(counted(&carrying(filtered(sensors(), STATUS_FAILS), &["neverRecorded"])), Vec::<i32>::new());
    }

    /// Two keyed terms contend, so their carriers are counted beside the
    /// Attributes `attrs` names, each once.
    #[test]
    fn the_census_counts_contended_terms_and_attrs_together_once() {
        let query = carrying(
            filtered(sensors(), "\"https://example.com/status\"==\"fail\";\"https://example.com/temperature\">1"),
            &["calibration", "temperature"],
        );
        assert_eq!(counted(&query), vec![12, 15, 16]);
    }
}
