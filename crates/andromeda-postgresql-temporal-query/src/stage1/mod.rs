//! Stage 1 of a temporal query: select the entity ids that match every filter.
//!
//! The statement drives from the temporal catalogue, never from `entities`.
//! Core Entity deletion (clause 5.6.6) removes the current-state row and leaves
//! the temporal evolution standing, so an entity's history has to stay
//! selectable after it; the entity URI and the entity types are carried on the
//! temporal catalogue for exactly that reason. The value filters read the
//! temporal evolution too: clause 5.7.4.4 checks `q` (step S2) and the geoquery
//! (step S3) against the Attribute instances inside the temporal window, which
//! are the history rows, never against current state. On the live branch every
//! one of those relations is the history source that also holds the entries
//! still queued, so a write is selected the moment it is acknowledged. A
//! selection by type alone walks the type memberships of the temporal
//! catalogue instead, so a page of a type visits that type's Evolutions in
//! identifier order and no other, and a selection by attribute with no type
//! walks the streams of its attributes the same way.
//!
//! The `datasetId` parameter selects no Entity. Clause 5.7.4.4 applies it at
//! step S8, after every selection step, to "filter the Attribute instances" and
//! "[r]emove all Attributes without remaining Attribute instances"; only step
//! S1 removes an Entity, and only for having no data in the window. So neither
//! the window match, nor `q`, nor the geoquery narrows by dataset here: an
//! Entity whose instances the parameter all filters out is selected, counted
//! and paged, and answers with no Attribute. The one criterion that reads the
//! parameter is a `pick` / `omit` projection's (`crate::projected_members`):
//! clause 5.7.4.5 reduces the Entity after step S8, so whether the projection
//! leaves it a member depends on the instances S8 kept.

mod candidate_step;
mod contributed_candidates;
mod entity_conditions;
mod evolution_walk;
mod extent_candidates;
mod gated_count;
mod gated_walk;
mod pattern_candidates;
#[cfg(test)]
mod query_fixture;
mod queued_candidates;
mod scope_candidates;
mod window_candidates;
mod window_match;

use crate::{
    alias::EntityIdColumn,
    stage1::{
        candidate_step::{CandidateStep, CountRead},
        entity_conditions::{PatternTest, build_entity_conditions_for_temporal},
        evolution_walk::{Among, Evolutions, evolution_rows},
        gated_count::gated_count,
        gated_walk::gated_walk,
        window_match::temporal_match_subquery,
    },
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    entity_ordering::{entity_identifier_key, ordering_collation},
    error::RenderError,
    history_source::{BesideComparison, HistoryBranch},
    page_window::apply_identifier_window,
};
use andromeda_postgresql_schema::{current::E, temporal::TemporalEntities};
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use sea_query::{Condition, Expr as SeaExpr, Func, SelectStatement, TableRef};

/// The Stage 1 statement and its optional count, each a statement tree the store
/// binds and executes on its own.
///
/// The result and the count carry the same entity predicate but different tails
/// (the result paginates and orders, the count neither), so each owns its own
/// placeholder sequence rather than sharing one, which is what a paginated
/// result and an unpaginated count cannot do.
pub struct Stage1Query {
    /// The paginated statement that selects the matching entity ids.
    pub select: SelectStatement,
    /// The unpaginated `COUNT`, present when clause 5.5.9 asks for a total.
    pub count: Option<SelectStatement>,
    /// The order the page's identifiers come back in, which the store presents
    /// ascending.
    pub read_order: ReadOrder,
    /// How PostgreSQL may plan `select`.
    pub planning: StatementPlanning,
    /// How PostgreSQL may plan `count`.
    pub count_planning: StatementPlanning,
}

/// Build Stage 1 query: find the identifiers `window` selects of the entities
/// matching entity-level filters, counting every match when `count` asks for
/// it.
///
/// # Errors
///
/// Returns [`RenderError::Query`] when the current-state filters cannot be
/// rendered: a `q` term (clause 4.9) naming an operator the store cannot apply
/// to the target path, or a geo query (clause 4.10) whose relation and geometry
/// combination clause 4.10 does not admit. Returns
/// [`RenderError::ReferenceGeometryEncoding`] when the geo query's reference
/// geometry cannot be encoded as `PostGIS` EWKB.
pub fn build_stage1_entity_ids(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    window: &PageWindow,
    count: CountPolicy,
    names: &QueryNames,
) -> Result<Stage1Query, RenderError> {
    if let Some(step) = CandidateStep::chosen(compiled, names)? {
        let (select, read_order) = gated_walk(branch, compiled, window, names, &step)?;
        let count_read = step.count_read();
        let count = match (count, count_read) {
            (CountPolicy::Emit, CountRead::Gated(bound)) => Some(gated_count(branch, compiled, names, &step, bound)?),
            (CountPolicy::Emit, CountRead::Walked) => Some(walked_count(branch, compiled, names)?),
            (CountPolicy::Omit, CountRead::Gated(_) | CountRead::Walked) => None,
        };
        return Ok(Stage1Query {
            select,
            count,
            read_order,
            planning: step.page_planning(),
            count_planning: count_read.planning(),
        });
    }
    let Evolutions { paged, type_test, .. } = evolution_rows(branch, compiled, names, Among::Every);
    // Entity-level conditions, applied to the Evolutions under alias "e".
    let entity_conditions = build_entity_conditions_for_temporal(branch, compiled, names, type_test, PatternTest::Tested)?;

    let mut select = selected_ids(
        paged,
        entity_conditions,
        temporal_match_subquery(branch, compiled, names, BesideComparison::Identifier),
    );

    // Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    // the JSON-LD array returned shall be ordered according to the member names
    // specified." Only `id` can be named (clause 5.7.4.3), and the collation is
    // written out rather than left to the column: `temporal_entities.id` is
    // declared `COLLATE "C"`, so an unstated comparison is byte-wise and the
    // collation Table 6.18.3.2-1 says the payload "shall be ordered according
    // to" would have no effect at all.
    //
    // Clause 5.5.9 asks for a consistent order across pages whether or not one
    // was requested, so the `id` follows as the tiebreaker of an offset page.
    // A cursor page (clause 5.5.9.1) walks the element dimension's default
    // `id` order alone: the pagination policy never pairs a cursor with a
    // requested ordering, which its identifier position cannot walk. The
    // window governs only the element dimension, never the temporal window
    // (`Content-Range`).
    match (&window.start, compiled.ordering.as_ref()) {
        (PageStart::Offset(_), Some(ordering)) => {
            entity_identifier_key(
                SeaExpr::col((E, TemporalEntities::Id)),
                ordering.direction,
                &ordering_collation(ordering.collation.as_ref()),
            )
            .order_by(&mut select);
        }
        (PageStart::Offset(_), None) | (PageStart::Cursor(_), _) => {}
    }
    let read_order = apply_identifier_window(&mut select, SeaExpr::col((E, TemporalEntities::Id)), window);

    let count_statement = match count {
        CountPolicy::Emit => Some(walked_count(branch, compiled, names)?),
        CountPolicy::Omit => None,
    };

    // A walk of the Evolutions takes one access path whatever values are
    // bound to it, so any plan the cache holds serves the page and the count.
    Ok(Stage1Query {
        select: select.take(),
        count: count_statement,
        read_order,
        planning: StatementPlanning::Cached,
        count_planning: StatementPlanning::Cached,
    })
}

/// How many Evolutions of the whole walk every filter and the step S1 window
/// match keep.
///
/// # Errors
///
/// Fails as rendering the entity conditions fails.
fn walked_count(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames) -> Result<SelectStatement, RenderError> {
    let Evolutions { counted, type_test, .. } = evolution_rows(branch, compiled, names, Among::Every);
    let conditions = build_entity_conditions_for_temporal(branch, compiled, names, type_test, PatternTest::Tested)?;
    Ok(counted_ids(
        counted,
        conditions,
        temporal_match_subquery(branch, compiled, names, BesideComparison::Key),
    ))
}

/// The identifiers, as `entity_id`, of the Evolutions of `evolutions` that
/// `conditions` and the step S1 `window_match` keep, unordered and unbounded.
fn selected_ids(evolutions: TableRef, conditions: Condition, window_match: SelectStatement) -> SelectStatement {
    let mut select = SelectStatement::new();
    select
        .expr_as(SeaExpr::col((E, TemporalEntities::Id)), EntityIdColumn)
        .from(evolutions)
        .cond_where(conditions)
        .cond_where(SeaExpr::exists(window_match));
    select
}

/// How many Evolutions of `evolutions` `conditions` and the step S1
/// `window_match` keep.
fn counted_ids(evolutions: TableRef, conditions: Condition, window_match: SelectStatement) -> SelectStatement {
    let mut count = SelectStatement::new();
    count
        .expr(Func::count(SeaExpr::col((E, TemporalEntities::Id))))
        .from(evolutions)
        .cond_where(conditions)
        .cond_where(SeaExpr::exists(window_match));
    count
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{PageBranches, built, built_count, built_windowed, ids, instant, minimal_compiled, point, sensor_names, sensors, speed_and_address},
    };
    use andromeda_ngsi_ld_geometry::geo_rel::GeoRel;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        dataset::DatasetSelector,
        entity_selection::EntitySelector,
        filter::parser,
        geo::query::GeoQuery,
        ordering::{collation::IcuCollation, sort_direction::SortDirection},
        page_window::{PageStart, PageWindow},
        temporal::{compiled::CompiledTemporalQuery, ordering::TemporalEntityOrdering, window::TimeInterval},
    };
    use andromeda_postgresql_query::{dictionary::query_names::QueryNames, history_source::HistoryBranch};
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Value, Values};

    /// The count statement of a counted query over the page `window` selects.
    fn built_windowed_count(compiled: &CompiledTemporalQuery, window: &PageWindow, names: &QueryNames) -> String {
        build_stage1_entity_ids(&HistoryBranch::live(), compiled, window, CountPolicy::Emit, names)
            .expect("valid temporal query")
            .count
            .expect("count requested")
            .build(PostgresQueryBuilder)
            .0
    }

    fn has_timestamp(values: &Values) -> bool {
        values.0.iter().any(|value| matches!(value, Value::ChronoDateTimeUtc(_)))
    }

    /// A requested ordering cuts the page in an order no candidates are read
    /// in, so the query takes no step.
    #[test]
    fn a_plain_stage1_is_planned_by_the_cache() {
        let mut compiled = sensors();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Ascending).build());
        let stage1 = build_stage1_entity_ids(
            &HistoryBranch::live_recorded(),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Emit,
            &sensor_names(),
        )
        .expect("valid temporal query");
        assert_eq!(stage1.planning, StatementPlanning::Cached);
        assert_eq!(stage1.count_planning, StatementPlanning::Cached);
    }

    #[test]
    fn entity_id_selection_orders_by_id_for_stable_pagination() {
        // The driving temporal catalogue emits one row per id, and ordered
        // paging keeps clause 5.5.9 result pages stable.
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("ORDER BY \"e\".\"id\""), "{sql}");
    }

    #[test]
    fn stage1_projects_the_entity_uri_as_entity_id() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"e\".\"id\" AS \"entity_id\""), "{sql}");
    }

    #[test]
    fn stage1_timerel_placeholder_is_offset_after_id_filters() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let (sql, values) = built(&compiled, &QueryNames::default());

        assert!(
            !sql.contains("< $1"),
            "time placeholder must not collide with earlier id-filter placeholders: {sql}"
        );
        assert!(!sql.contains("::timestamptz"), "{sql}");
        assert!(has_timestamp(&values), "the window instant is bound: {values:?}");
    }

    #[test]
    fn stage1_count_reuses_the_result_predicate_bind_order() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let (_, values) = built(&compiled, &QueryNames::default());
        let count_sql = built_count(&compiled, &QueryNames::default());

        assert!(count_sql.contains("EXISTS(SELECT"), "{count_sql}");
        assert!(count_sql.contains("$1"), "{count_sql}");
        assert!(count_sql.contains("$2"), "{count_sql}");
        assert!(has_timestamp(&values), "the window instant is bound: {values:?}");
    }

    /// Clause 5.7.4.4: the `datasetId` parameter is step S8, which filters
    /// Attribute instances after every selection step and removes no Entity.
    /// Neither the step S1 window match, nor the step S2 `q`, nor the step S3
    /// geoquery, nor the count names a dataset, for every form of the
    /// parameter.
    #[test]
    fn no_selection_step_narrows_by_the_dataset_parameter() {
        for selectors in [&["@none"][..], &["urn:ngsi-ld:Dataset:one"], &["@none", "urn:ngsi-ld:Dataset:one"]] {
            let mut compiled = minimal_compiled();
            compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
            compiled.instances.dataset_id_filter = selectors
                .iter()
                .map(|selector| DatasetSelector::parse(selector).expect("a valid dataset selector"))
                .collect();
            compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">100").unwrap());
            compiled.entities.geo = Some(GeoQuery {
                georel: GeoRel::Within,
                geometry: point(&serde_json::json!([1.0, 2.0])),
                geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
            });
            let (sql, _) = built(&compiled, &speed_and_address());
            assert!(
                !sql.contains(r#""dataset_id" IN"#) && !sql.contains(r#""dataset_id" IS NULL"#),
                "{selectors:?}: {sql}"
            );
            let count = built_count(&compiled, &speed_and_address());
            assert!(
                !count.contains(r#""dataset_id" IN"#) && !count.contains(r#""dataset_id" IS NULL"#),
                "{selectors:?}: {count}"
            );
        }
    }

    /// Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    /// the JSON-LD array returned shall be ordered according to the member
    /// names specified", and Table 6.18.3.2-1 makes the collation the
    /// comparison the payload "shall be ordered according to". The identifier
    /// column is `COLLATE "C"`, so the requested collation has to be written
    /// into the key or the ordering is byte-wise whatever was asked for.
    #[test]
    fn a_requested_ordering_collates_the_identifier_key() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering {
            direction: SortDirection::Ascending,
            collation: Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "de-u-co-phonebk" ASC NULLS LAST"#), "{sql}");
    }

    /// Clause 4.23.1 makes ICU "root" (`und-x-icu`) the default collation
    /// order, so an ordering that named none still names one rather than
    /// falling back to the column's byte-wise `COLLATE "C"`.
    #[test]
    fn an_ordering_without_a_collation_compares_under_the_icu_root_order() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Ascending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "und-x-icu" ASC NULLS LAST"#), "{sql}");
    }

    /// Table 6.18.3.2-1 gives `desc` as one of the two sorting styles.
    #[test]
    fn a_descending_ordering_reverses_the_identifier_key() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Descending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "und-x-icu" DESC NULLS LAST"#), "{sql}");
    }

    /// The clause 5.5.9 tiebreaker survives an explicit ordering, so a page
    /// boundary never falls inside a group the requested key left tied.
    #[test]
    fn the_stable_identifier_tiebreaker_follows_a_requested_ordering() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Descending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        let collated = sql.find("COLLATE").expect("the requested key is written");
        let tiebreaker = sql.rfind(r#""e"."id" ASC"#).expect("the stable key is written");
        assert!(collated < tiebreaker, "{sql}");
    }

    /// Table 6.18.3.2-1 makes `collation` "part of Entity ordering", so a query
    /// that ordered nothing keeps the store's own stable order and pays for no
    /// collation at all.
    #[test]
    fn a_query_that_requested_no_ordering_keeps_the_stable_identifier_order() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(!sql.contains("COLLATE"), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" ASC"#), "{sql}");
    }

    /// The count answers "how many", which no ordering changes, so clause 5.5.9
    /// gets its total without a sort the planner would have to perform.
    #[test]
    fn the_count_statement_carries_no_ordering() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Ascending).build());
        let sql = built_count(&compiled, &QueryNames::default());
        let outer = sql.rsplit(r#") AS "e""#).next().expect("the statement past its entity source");
        assert!(!outer.contains("ORDER BY"), "{sql}");
    }

    fn boundary(id: &str) -> Identifier {
        id.parse().expect("valid URI")
    }

    /// A cursor page of 20 starting at `cursor`.
    fn cursor_page(cursor: PageCursor) -> PageWindow {
        PageWindow {
            limit: 20,
            start: PageStart::Cursor(Some(cursor)),
        }
    }

    /// Clause 5.5.9.1: a forward cursor pages the element dimension past the
    /// boundary id over the default order -- `WHERE id > $x ORDER BY id ASC` and
    /// no `OFFSET`.
    #[test]
    fn stage1_keyset_after_pages_forward_ascending_without_offset() {
        let compiled = minimal_compiled();
        let (sql, _) = built_windowed(&compiled, &PageWindow::after(20, boundary("urn:ngsi-ld:Vehicle:001")), &QueryNames::default());
        assert!(sql.contains(r#""e"."id" > $"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" ASC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "a keyset page never skips: {sql}");
    }

    /// Clause 5.5.9.1: a backward cursor pages before the boundary over the
    /// reversed order -- `WHERE id < $x ORDER BY id DESC` -- which the store
    /// reverses to ascending after fetching.
    #[test]
    fn stage1_keyset_before_pages_backward_descending_without_offset() {
        let compiled = minimal_compiled();
        let (sql, _) = built_windowed(
            &compiled,
            &cursor_page(PageCursor::Before(boundary("urn:ngsi-ld:Vehicle:042"))),
            &QueryNames::default(),
        );
        assert!(sql.contains(r#""e"."id" < $"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" DESC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// Clause 5.5.9.1: the from-end cursor takes the final page -- `ORDER BY id
    /// DESC`, no boundary predicate -- which the store reverses to ascending.
    #[test]
    fn stage1_keyset_end_takes_the_final_page_descending() {
        let compiled = minimal_compiled();
        let (sql, _) = built_windowed(&compiled, &cursor_page(PageCursor::End), &QueryNames::default());
        assert!(sql.contains(r#"ORDER BY "e"."id" DESC"#), "{sql}");
        assert!(!sql.contains(r#""e"."id" >"#) && !sql.contains(r#""e"."id" <"#), "no boundary predicate: {sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// The count statement ignores the keyset, so a counted cursor page still
    /// reports the whole collection's total rather than the remaining tail.
    #[test]
    fn a_keyset_count_still_counts_the_whole_collection() {
        let compiled = minimal_compiled();
        let sql = built_windowed_count(&compiled, &PageWindow::after(20, boundary("urn:ngsi-ld:Vehicle:001")), &QueryNames::default());
        assert!(!sql.contains(r#""e"."id" >"#), "the count is not narrowed by the cursor: {sql}");
    }

    /// Clause 6.3.13: the count is the number of entities the page is taken
    /// from, so the count reads the walk the page falls to, with the same
    /// window match.
    #[test]
    fn the_count_reads_the_same_source_as_the_page() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let stage1 = build_stage1_entity_ids(
            &HistoryBranch::live(),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Emit,
            &QueryNames::default(),
        )
        .expect("valid temporal query");
        let page = stage1.select.to_string(PostgresQueryBuilder);
        let count = stage1.count.expect("count requested").to_string(PostgresQueryBuilder);
        let walk = PageBranches::of(&page).walk.replace(
            r#"(CASE WHEN ((SELECT COUNT(*) FROM "short_walk") >= 20) THEN FALSE WHEN ((SELECT COUNT(*) FROM "short_walk") >= 3) THEN TRUE ELSE (SELECT COUNT(*) FROM "candidates") > 10000 END) AND "#,
            "",
        );
        let source = |sql: &str, head: &str| {
            let from = sql.find(head).unwrap_or_else(|| panic!("{head} in {sql}")) + head.len();
            let tail = sql[from..].find(r#" ORDER BY "e"."id""#).map_or(sql.len(), |at| from + at);
            sql[from..tail].to_owned()
        };
        assert_eq!(source(&walk, r#"AS "entity_id" FROM "#), source(&count, r#"COUNT("e"."id") FROM "#));
    }
}
