//! The page of an entity query filtered by a geo-query (clause 4.10), read in
//! identifier order (clause 5.5.9) with no total on its rows.
//!
//! The plain page narrows the selection to every Entity the spatial index
//! yields, sorted by key, and reads every match before it cuts the first row.
//! Among 500 000 Entities a geo-query nearly all of them meet takes 0.8 to
//! 1.0 s that way, and one a tenth of them meet takes 122 ms. The planner
//! cannot price the geo-query against a walk of the selection, so the page is
//! a [`gated_page`]:
//!
//! 1. the short walk probes the first [`GEO_SHORT_WALK`] Entities of the
//!    selection in identifier order for the geo-query, and for `q` when the
//!    query has one;
//! 2. the index reads at most [`GEO_CANDIDATE_BOUND`] + 1 matches through the
//!    spatial indexes, and the page is cut from the selection narrowed to
//!    them, under every other filter;
//! 3. the full walk probes every Entity of the selection in identifier order,
//!    when the matches pass the bound.
//!
//! A broad geo-query fills its page within the short walk, in 0.4 ms. One a
//! tenth of the Entities meet reads the bound's matches and walks, in 8.3 ms.
//! A rare geo-query pays the short walk and a costlier plan before its
//! matches answer, 0.8 ms for 111 matches where the plain page takes 0.5 ms,
//! and one a hundredth of the Entities meet costs what the plain page costs,
//! within its spread from run to run.
//!
//! The walks probe each Entity they visit by its key, the hoisted default
//! `location` through the search row's primary key and every other instance
//! through the Entity's attribute rows, each box tested without a spatial
//! index. A probe costs about a microsecond beside the type-membership walk
//! and 1.6 to 1.7 µs beside `entities`, so the walks take the membership walk
//! when the selectors are types alone ([`EntityRelations::for_geo_walk`]). The
//! index branch keeps `entities` driving, narrowed to the matches, each looked
//! up by its key.
//!
//! Only a page whose rows carry no total is cut this way. Counting the whole
//! selection costs about what the plain page costs, 0.8 to 1.0 s for a
//! geo-query nearly every Entity meets, so a counted page that can carry its
//! total stays plain rather than add a gated page to a count it cannot avoid.
//! A `q` with a keyed term is cut by the keyed page, which reads the
//! geo-query the same way ahead of its keyed terms: its walks are a
//! [`GeoWalk`] probed by [`geo_probe`], and its first read is [`geo_read`].
//!
//! After the matches the page reads the filters its walks test on each Entity
//! ([`filter_reads`](crate::filter_reads)): the carriers of an `attrs` the
//! census bounds, else the matches of a `scopeQ`. Within all of 500 000
//! scoped Entities beside a scope none of them holds, the page answers in
//! 4.5 ms where the walk takes 629 ms.

use crate::{
    entity::{GeoFilter, QFilter},
    entity_page::ProjectedPage,
    entity_relations::EntityRelations,
    filter_reads::filter_read_before,
    gated_page::{GatedColumns, IndexRead, PageSource, gated_page},
    page_shape::carried_on_rows,
    page_walk::{PageWalk, walked_rows},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    entity_selection::ConsideredEntities,
    filter::compiled_filter::CompiledFilter,
    geo::compiled_geo_query::CompiledGeoQuery,
    page_window::PageWindow,
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    geo_predicate as geo_sql,
    one_time_gate::CandidateBound,
    q_predicate::{instance_scan::InstanceScan, keyed_term::CensusCount, projection::compile},
    query_names::QueryNames,
};
use sea_query::{Condition, SelectStatement};
use std::iter::once;

/// How many Entities the short walk probes before the spatial index is read.
///
/// A geo-query half the Entities meet fills a page of a hundred within it. A
/// probe of one Entity's instances costs about a microsecond, so a walk that
/// falls short costs 0.2 to 0.4 ms before the matches are read, which a rare
/// geo-query pays beside its read.
pub(crate) const GEO_SHORT_WALK: u32 = 200;

/// The most matches the index step reads before the selection is walked.
///
/// The spatial indexes yield 10 000 matches in about 7 ms. The bound trades
/// two placements of the matches, which no count tells apart before the read.
/// Matches holding the last identifiers of the selection make the walk probe
/// every Entity, about 0.5 s among 500 000 members of a type, where the index
/// answers the bound's matches. Matches spread through the selection fill the
/// page soon into the walk, so a geo-query a tenth of 500 000 Entities meet
/// reads the bound's matches and walks, in 8.3 ms against 122 ms for the
/// plain page.
pub(crate) const GEO_CANDIDATE_BOUND: u64 = 10_000;

/// A geo-query and the Entities its matches name.
pub(crate) struct GeoMatches<'q> {
    /// The geo-query each walked Entity is probed for.
    pub(crate) geo: &'q CompiledGeoQuery,
    /// `SELECT "entity_key" …` of every instance of the geo-query's
    /// `GeoProperty` that meets it, read through the spatial indexes, once per
    /// matching instance and in no order.
    pub(crate) candidates: SelectStatement,
}

/// The matches of the query's geo-query, read in the branch and at the
/// instant of `rows`, when the geo page cuts the page.
///
/// That is when the selection holds a geo-query, considers some Entity and
/// has no `orderBy`, the page's rows carry no total (`count` omitted, or a
/// page past a cursor), and the geo-query has matches to read. A `GeoProperty`
/// the dictionary never recorded, or a `location` with no arm that can hold a
/// match, leaves the plain page its constant `FALSE`. A `q` with a keyed term
/// is cut by the keyed page, which is tried first.
///
/// # Errors
///
/// Fails as rendering the geo-query fails: a reference geometry with no EWKB
/// spelling, or `georel=near` naming no distance.
pub(crate) fn geo_matches<'q>(
    query: &'q CompiledEntityQuery,
    names: &QueryNames,
    window: &PageWindow,
    count: CountPolicy,
    rows: &EntityRowRefs,
) -> Result<Option<GeoMatches<'q>>, RenderError> {
    match geo_page_query(query, window, count) {
        Some(_) => matches_of(query, names, rows),
        None => Ok(None),
    }
}

/// The geo-query of `query` when a page over `window` whose total `count`
/// asks for can be a geo page: the selection holds a geo-query and considers
/// some Entity, and the page has no `orderBy` and carries no total on its
/// rows. Whether the geo-query has matches to read is left to
/// [`matches_of`].
pub(crate) fn geo_page_query<'q>(query: &'q CompiledEntityQuery, window: &PageWindow, count: CountPolicy) -> Option<&'q CompiledGeoQuery> {
    let selection = &query.selection;
    let considered = match selection.considered {
        ConsideredEntities::Selected => true,
        ConsideredEntities::NoEntity => false,
    };
    let uncounted = match carried_on_rows(count, window) {
        CountPolicy::Emit => false,
        CountPolicy::Omit => true,
    };
    selection.geo.as_ref().filter(|_| considered && uncounted && query.ordering.terms.is_empty())
}

/// The matches of the query's geo-query, read in the branch and at the
/// instant of `rows`, when the selection holds a geo-query, considers some
/// Entity, and the geo-query has matches to read: a `GeoProperty` the
/// dictionary never recorded, or a `location` with no arm that can hold a
/// match, has none.
///
/// # Errors
///
/// Fails as rendering the geo-query fails: a reference geometry with no EWKB
/// spelling, or `georel=near` naming no distance.
pub(crate) fn matches_of<'q>(query: &'q CompiledEntityQuery, names: &QueryNames, rows: &EntityRowRefs) -> Result<Option<GeoMatches<'q>>, RenderError> {
    let selection = &query.selection;
    let Some(geo) = &selection.geo else {
        return Ok(None);
    };
    match selection.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => return Ok(None),
    }
    let candidates = geo_sql::matches(geo, rows, &selection.default_location, &names.attributes)?;
    Ok(candidates.map(|candidates| GeoMatches { geo, candidates }))
}

/// The relations a walk reads when it probes the geo-query on each Entity it
/// yields ([`EntityRelations::for_geo_walk`]), and how its fragments reach
/// their rows.
pub(crate) struct GeoWalk {
    /// The relations the walk reads.
    relations: EntityRelations,
    /// How the walk's fragments reach the rows of `relations`.
    rows: EntityRowRefs,
}

impl GeoWalk {
    /// The geo walk of the query `source` pages, in its branch and standing at
    /// its instant.
    pub(crate) fn of(source: &PageSource<'_>) -> GeoWalk {
        let relations = EntityRelations::for_geo_walk(source.query, &source.names.types, source.schema);
        let rows = relations.rows(source.schema.cloned(), source.rows.standing());
        GeoWalk { relations, rows }
    }

    /// `source` read through the walk's relations.
    pub(crate) const fn source<'a>(&'a self, source: &PageSource<'a>) -> PageSource<'a> {
        PageSource {
            relations: &self.relations,
            rows: &self.rows,
            ..*source
        }
    }
}

/// The probe of a walk of `source` that leaves `geo` to each Entity it
/// yields: `q`, when given, and `geo`, each over [`walked_rows`].
///
/// # Errors
///
/// Fails as rendering the geo-query fails: a reference geometry with no EWKB
/// spelling, or `georel=near` naming no distance.
pub(crate) fn geo_probe(source: &PageSource<'_>, geo: &CompiledGeoQuery, q: Option<&CompiledFilter>) -> Result<Condition, RenderError> {
    let walked = walked_rows(source.rows);
    let q = q.map(|q| compile(q.reading(), &InstanceScan::Walked(&walked), source.names));
    Ok(Condition::all().add_option(q).add(geo_sql::probe(geo, &walked, &source.names.attributes)?))
}

/// The read of a geo-query's `matches` for a page of `source`: at most
/// [`GEO_CANDIDATE_BOUND`] of them, the page cut from them under every filter
/// but the geo-query they answer.
///
/// # Errors
///
/// Fails as rendering the plain page's filter fails: a reference geometry
/// with no EWKB spelling, or a query-language rule a renderer decides first.
pub(crate) fn geo_read(source: &PageSource<'_>, matches: GeoMatches<'_>) -> Result<IndexRead, RenderError> {
    Ok(IndexRead {
        candidates: matches.candidates,
        bound: CandidateBound::AtMost(GEO_CANDIDATE_BOUND),
        narrowed: source.condition(QFilter::Whole, GeoFilter::Omitted)?,
    })
}

/// The projected page of `source`, filtered by a geo-query, cut from its
/// `matches`, and the order its rows come back in, its rows carrying what
/// `columns` asks for.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn geo_page(source: &PageSource<'_>, matches: GeoMatches<'_>, columns: GatedColumns) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let geo_walk = GeoWalk::of(source);
    let walked_source = geo_walk.source(source);
    let walk = PageWalk {
        source: walked_source,
        walked: walked_source.conditions(QFilter::Omitted, GeoFilter::Omitted)?,
        probe: geo_probe(source, matches.geo, source.query.selection.q.as_ref())?,
        short: GEO_SHORT_WALK,
    };
    let reads = once(geo_read(source, matches)?)
        .chain(filter_read_before(source, GeoFilter::Probed, Vec::new(), CensusCount::Unbounded)?)
        .collect();
    gated_page(source, walk, reads, columns)
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_total::EntityTotal,
        geo_page_fixtures::{OBSERVATION_SPACE, built, filtered, first_page, iri, past_first_sensor, sensors_within_square},
    };
    use andromeda_ngsi_ld_query::count_policy::CountPolicy;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_geo_page_walks_the_type_members_and_probes_each_for_the_relation() {
        assert_statement_snapshot!(
            "geo-page-a-geo-page-walks-the-type-members-and-probes-each-for-the-relation",
            built(&sensors_within_square(), &first_page(), CountPolicy::Omit).select
        );
    }

    /// The walk reads `entities` alone, since the probe reads each Entity's
    /// own instances and no other filter reads the search row.
    #[test]
    fn a_geo_page_without_a_type_walks_every_entity_in_identifier_order() {
        let mut query = sensors_within_square();
        query.selection.selectors = Vec::new();
        assert_statement_snapshot!(
            "geo-page-a-geo-page-without-a-type-walks-every-entity-in-identifier-order",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    #[test]
    fn attrs_beside_a_geo_query_are_read_from_the_search_row_of_each_walked_member() {
        let mut query = sensors_within_square();
        query.selection.attrs = vec![iri("temperature")];
        assert_statement_snapshot!(
            "geo-page-attrs-beside-a-geo-query-are-read-from-the-search-row-of-each-walked-member",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    /// No index answers `!=`, so the walks probe `q` beside the relation and
    /// the index branch filters its matches by it.
    #[test]
    fn an_unkeyed_q_is_probed_beside_the_relation_and_filters_the_index_branch() {
        let query = filtered(sensors_within_square(), "\"https://example.com/status\"!=\"fail\"");
        assert_statement_snapshot!(
            "geo-page-an-unkeyed-q-is-probed-beside-the-relation-and-filters-the-index-branch",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    #[test]
    fn a_custom_geoproperty_page_reads_its_instance_matches() {
        let mut query = sensors_within_square();
        query.selection.geo.as_mut().expect("a geo-query").geoproperty = OBSERVATION_SPACE.parse().expect("valid attribute name");
        assert_statement_snapshot!(
            "geo-page-a-custom-geoproperty-page-reads-its-instance-matches",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    #[test]
    fn a_continued_cursor_page_walks_past_its_cursor() {
        assert_statement_snapshot!(
            "geo-page-a-continued-cursor-page-walks-past-its-cursor",
            built(&sensors_within_square(), &past_first_sensor(), CountPolicy::Omit).select
        );
    }

    /// Clause 6.3.13: past a cursor the total is a count of its own, over the
    /// whole filter, planned like any other.
    #[test]
    fn a_geo_page_is_planned_per_request_and_counts_as_before() {
        let uncounted = built(&sensors_within_square(), &first_page(), CountPolicy::Omit);
        assert_eq!(uncounted.planning, StatementPlanning::GatedBranches);
        assert!(matches!(uncounted.total, EntityTotal::Omitted));
        let counted = built(&sensors_within_square(), &past_first_sensor(), CountPolicy::Emit);
        assert_eq!(counted.planning, StatementPlanning::GatedBranches);
        assert!(matches!(counted.total, EntityTotal::Counted(_)));
        assert_eq!(counted.total_planning, StatementPlanning::Cached);
    }
}
