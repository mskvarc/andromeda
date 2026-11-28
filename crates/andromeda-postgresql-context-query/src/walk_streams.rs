//! A page of a type walk of several alternatives, read one stream at a time.
//!
//! The walk of several alternatives is one stream per alternative, each a
//! range of the membership index in identifier order, holding the Entities
//! no alternative before it selects. PostgreSQL merges no two ranges of one
//! index in order, so a page ordered over their union sorts every member of
//! the types before its first row: among the 757 134 members of two types of
//! a GTFS feed it spills a 47 MB sort and takes 260 ms for a page of one.
//! Each Entity is in one stream, so the first `n` rows of the selection in
//! page order are among the first `n` of each stream in that order. A page of
//! `limit` rows past `offset` therefore reads each stream in page order up to
//! `offset + limit` rows under the page's filters, and sorts and cuts only
//! their union. A cursor bounds every stream the same way, each read past the
//! cursor in the cursor's direction.
//!
//! A page counted by `COUNT(*) OVER ()` reads the whole union instead: the
//! window counts every row the filters keep before the page is cut, and the
//! union then sorts the matches alone.

use crate::{
    entity_page::{PageTotal, ProjectedPage, TOTAL_COUNT_COLUMN},
    entity_relations::{EntityRelations, StreamCount},
    gated_page::{PageSource, cut},
};
use andromeda_ngsi_ld_query::{
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{error::RenderError, one_time_gate::wrapped};
use andromeda_postgresql_seaquery::scalar_subquery::scalar_subquery;
use sea_query::{Alias, Condition, Expr, SelectStatement, UnionType};

/// The page cut from the selection read through `relations`, filtered by
/// `condition`, the rows of `window` past its first `skipped`, projected and
/// carrying `total`, with the order it is read in.
///
/// A type walk of several alternatives is read stream by stream and the
/// pages of the streams merged ([`merged`]), unless `total` counts the page's
/// rows before the window cuts them.
///
/// # Errors
///
/// Fails as rendering the page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn streamed_page(
    source: &PageSource<'_>,
    relations: EntityRelations,
    condition: Condition,
    window: &PageWindow,
    skipped: u64,
    total: PageTotal,
) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    match (relations.stream_count(), total) {
        (StreamCount::Several, total @ (PageTotal::Omitted | PageTotal::Counted(_))) => {
            let (first, rest) = relations.streams();
            let (first, read_order) = source.page_through(first, condition.clone(), window, PageTotal::Omitted)?;
            let rest = rest
                .into_iter()
                .map(|stream| Ok(source.page_through(stream, condition.clone(), window, PageTotal::Omitted)?.0))
                .collect::<Result<Vec<ProjectedPage>, RenderError>>()?;
            let mut page = merged(first, rest, window, skipped);
            match total {
                PageTotal::Counted(count) => {
                    page.select.expr_as(scalar_subquery(*count), Alias::new(TOTAL_COUNT_COLUMN));
                }
                PageTotal::Omitted | PageTotal::Windowed => {}
            }
            Ok((page, read_order))
        }
        (StreamCount::One, total) | (StreamCount::Several, total @ PageTotal::Windowed) => {
            let (mut page, read_order) = source.page_through(relations, condition, window, total)?;
            cut(&mut page.select, window, skipped);
            Ok((page, read_order))
        }
    }
}

/// `first` and `rest`, the page of each stream of a type walk ordered as the
/// page is, merged in that order and cut to the rows of `window` past its
/// first `skipped`.
///
/// The page of each stream is read from where `window` starts up to the last
/// of those rows, and only their union is sorted. A walk of one stream is cut
/// on its own.
pub(crate) fn merged(first: ProjectedPage, rest: Vec<ProjectedPage>, window: &PageWindow, skipped: u64) -> ProjectedPage {
    let ProjectedPage { select: mut first, order } = first;
    if rest.is_empty() {
        cut(&mut first, window, skipped);
        return ProjectedPage { select: first, order };
    }
    let reached = reach(window, skipped);
    let stream = |mut page: SelectStatement| {
        page.reset_offset().limit(reached);
        wrapped(page, "stream")
    };
    let mut union = stream(first);
    for page in rest {
        union.union(UnionType::All, stream(page.select));
    }
    let mut merged = wrapped(union, "streams");
    for (column, key_order) in &order {
        key_order.order_by(&mut merged, Expr::col(column.clone()));
    }
    cut(&mut merged, window, skipped);
    ProjectedPage { select: merged, order }
}

/// How many rows of a stream in page order the rows of `window` past its
/// first `skipped` reach from where it starts: its offset, `skipped` and its
/// limit.
fn reach(window: &PageWindow, skipped: u64) -> u64 {
    let offset = match window.start {
        PageStart::Offset(offset) => u64::from(offset),
        PageStart::Cursor(_) => 0,
    };
    offset.saturating_add(skipped).saturating_add(u64::from(window.limit))
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::build_ids_query,
        entity_page::ProjectedPage,
        geo_page_fixtures::{built, filtered, first_page, names, sensors, sensors_and_gateways},
        walk_streams::{merged, reach},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        geo::compiled_geo_query::CompiledGeoQuery,
        page_window::{PageStart, PageWindow},
        scope::scope_query::ScopeQuery,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Query};

    fn boundary() -> Identifier {
        "urn:ngsi-ld:Sensor:s9".parse().expect("valid URI")
    }

    /// A page of 100 past 1 000 Entities of two types reads 1 100 rows of
    /// each type's stream in identifier order, and sorts their union alone.
    #[test]
    fn a_later_page_of_two_types_reads_each_stream_up_to_its_offset_and_limit() {
        assert_statement_snapshot!(
            "walk-streams-a-later-page-of-two-types-reads-each-stream-up-to-its-offset-and-limit",
            built(&sensors_and_gateways(), &PageWindow::offset(100, 1000), CountPolicy::Omit).select
        );
    }

    /// Clause 5.5.9.1: a cursor page reads each stream past the cursor in
    /// the cursor's direction, a backward page descending from it.
    #[test]
    fn a_cursor_page_of_two_types_reads_each_stream_past_its_cursor() {
        let before = PageWindow {
            limit: 100,
            start: PageStart::Cursor(Some(PageCursor::Before(boundary()))),
        };
        for (direction, window) in [("after", PageWindow::after(100, boundary())), ("before", before)] {
            assert_statement_snapshot!(
                format!("walk-streams-a-cursor-page-of-two-types-reads-each-stream-past-its-cursor-{direction}"),
                built(&sensors_and_gateways(), &window, CountPolicy::Omit).select
            );
        }
    }

    /// A filtered page whose window counts it reads the whole union of the
    /// streams: the window counts every match before the page is cut.
    #[test]
    fn a_filtered_page_of_two_types_counted_by_its_window_reads_their_union() {
        let mut query = sensors_and_gateways();
        query.selection.scope_q = Some("/Madrid".parse::<ScopeQuery>().expect("scopeQ parses"));
        assert_statement_snapshot!(
            "walk-streams-a-filtered-page-of-two-types-counted-by-its-window-reads-their-union",
            built(&query, &first_page(), CountPolicy::Emit).select
        );
    }

    /// A plain page over two types runs without parallel workers, and over
    /// one type on any plan the plan cache chooses.
    #[test]
    fn a_plain_page_of_two_types_runs_without_parallel_workers() {
        assert_eq!(
            built(&sensors_and_gateways(), &first_page(), CountPolicy::Omit).planning,
            StatementPlanning::BoundedStreams
        );
        assert_eq!(built(&sensors(), &first_page(), CountPolicy::Omit).planning, StatementPlanning::Cached);
    }

    /// The identifiers of two types are read stream by stream, as a page is,
    /// without parallel workers.
    #[test]
    fn the_identifiers_of_two_types_are_read_stream_by_stream() {
        let ids = build_ids_query(&ContextTarget::Live, &sensors_and_gateways(), &PageWindow::offset(20, 40), &names()).expect("valid ids query");
        assert_statement_snapshot!("walk-streams-the-identifiers-of-two-types-are-read-stream-by-stream", ids.select);
        assert_eq!(ids.planning, StatementPlanning::BoundedStreams);
    }

    /// The short walk of a keyed page over two types reads at most its
    /// length of each stream, the continued walk each stream up to the end
    /// of its span, and the full walk walks each stream on its own up to the
    /// matches the page needs.
    #[test]
    fn a_keyed_page_of_two_types_walks_each_stream() {
        let query = filtered(sensors_and_gateways(), "\"https://example.com/status\"==\"fail\"");
        assert_statement_snapshot!(
            "walk-streams-a-keyed-page-of-two-types-walks-each-stream",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    /// The walks of a geo page over two types read the streams as a keyed
    /// page's do, each Entity probed for the geo-query.
    #[test]
    fn a_geo_page_of_two_types_walks_each_stream() {
        let mut query = sensors_and_gateways();
        query.selection.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({
                "type": "Polygon",
                "coordinates": [[[0.0, 40.0], [10.0, 40.0], [10.0, 50.0], [0.0, 50.0], [0.0, 40.0]]],
            }))
            .expect("square parses"),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        assert_statement_snapshot!(
            "walk-streams-a-geo-page-of-two-types-walks-each-stream",
            built(&query, &first_page(), CountPolicy::Omit).select
        );
    }

    /// One stream is cut on its own, past the window's offset and the
    /// skipped rows.
    #[test]
    fn a_walk_of_one_stream_is_cut_on_its_own() {
        let stream = || Query::select().column(Alias::new("entity_key")).from(Alias::new("selected")).take();
        let page = ProjectedPage {
            select: stream(),
            order: Vec::new(),
        };
        let mut expected = stream();
        expected.offset(45).limit(20);
        assert_eq!(merged(page, Vec::new(), &PageWindow::offset(20, 40), 5).select, expected);
    }

    /// Each stream is read up to the window's offset, the skipped rows and
    /// its limit, from where the window starts.
    #[test]
    fn a_stream_is_read_up_to_the_last_row_its_window_reaches() {
        assert_eq!(reach(&PageWindow::offset(100, 1000), 0), 1100);
        assert_eq!(reach(&PageWindow::after(100, boundary()), 1000), 1100);
        assert_eq!(reach(&PageWindow::offset(u32::MAX, u32::MAX), u64::MAX), u64::MAX);
    }
}
