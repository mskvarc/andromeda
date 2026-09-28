//! Stage 1 of a temporal query: select the entity ids that match every filter.
//!
//! The statement drives from `temporal_entities`, never from `entities`. Core
//! Entity deletion (clause 5.6.6) removes the current-state row and leaves the
//! temporal evolution standing, so an entity's history has to stay selectable
//! after it; the entity URI and the entity types are carried on the temporal
//! catalogue for exactly that reason. The value filters read the temporal
//! evolution too: clause 5.7.4.4 checks `q` (step S2) and the geoquery (step S3)
//! against the Attribute instances inside the temporal window, which are the
//! `attribute_history` rows, never against current state.
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

use crate::{
    alias::{Ah, Ahg, An, Ang, EntityIdColumn, S, Sg},
    axis::axis_column,
    projected_members::projected_member_condition,
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::{
    cursor::PageCursor,
    entity_selection::{ConsideredEntities, EntitySelector},
    pagination::CountPolicy,
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_postgresql_query::{
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    entity_ordering::{order_by_entity_identifier, ordering_collation},
    error::RenderError,
    geo_predicate::relation_expression,
    q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, projection as q_filter},
    scope_predicate::{ScopeTarget, condition as scope_condition},
    time_interval_predicate::within_interval,
    type_predicate::condition as type_condition,
};
use andromeda_postgresql_schema::{
    current::E,
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    cast::cast,
    literal::{false_literal, int_literal},
    operator::regex_match,
    pg_type::PostgresType,
};
use iri_rs::IriBuf;
use sea_query::{Condition, DynIden, Expr as SeaExpr, ExprTrait, Func, JoinType, Order, SelectStatement};

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
}

/// Build Stage 1 query: find entity IDs matching entity-level filters.
///
/// # Errors
///
/// Returns [`RenderError::Query`] when the current-state filters cannot be
/// rendered: a `q` term (clause 4.9) naming an operator the store cannot apply
/// to the target path, or a geo query (clause 4.10) whose relation and geometry
/// combination clause 4.10 does not admit. Returns
/// [`RenderError::ReferenceGeometryEncoding`] when the geo query's reference
/// geometry cannot be encoded as `PostGIS` EWKB.
pub fn build_stage1_entity_ids(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery, names: &QueryNames) -> Result<Stage1Query, RenderError> {
    // Entity-level conditions, applied to `temporal_entities` under alias "e".
    let entity_conditions = build_entity_conditions_for_temporal(schema, compiled, names)?;

    let mut select = SelectStatement::new();
    select
        .expr_as(SeaExpr::col((E, TemporalEntities::Id)), EntityIdColumn)
        .from(qualified(schema, TemporalEntities::Table).alias(E))
        .cond_where(entity_conditions.clone()) // clone: the count query applies the same entity predicate
        .cond_where(SeaExpr::exists(temporal_match_subquery(schema, compiled)));

    // Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    // the JSON-LD array returned shall be ordered according to the member names
    // specified." Only `id` can be named (clause 5.7.4.3), and the collation is
    // written out rather than left to the column: `temporal_entities.id` is
    // declared `COLLATE "C"`, so an unstated comparison is byte-wise and the
    // collation Table 6.18.3.2-1 says the payload "shall be ordered according
    // to" would have no effect at all.
    match &compiled.keyset {
        None => {
            if let Some(ordering) = compiled.ordering.as_ref() {
                order_by_entity_identifier(
                    &mut select,
                    SeaExpr::col((E, TemporalEntities::Id)),
                    ordering.direction,
                    &ordering_collation(ordering.collation.as_ref()),
                );
            }

            // Clause 5.5.9 asks for a consistent order across pages whether or not
            // one was requested: `LIMIT`/`OFFSET` over an otherwise unordered
            // result can repeat and skip rows between pages.
            select
                .order_by((E, TemporalEntities::Id), Order::Asc)
                .limit(u64::from(compiled.pagination.limit))
                .offset(u64::from(compiled.pagination.offset));
        }
        // Clause 5.5.9.1: keyset over the element dimension's default `id` order.
        // Restricted to the default order, so no requested `orderBy` is present
        // and the `OFFSET` gives way to the direction-appropriate predicate.
        Some(cursor) => {
            apply_stage1_keyset(&mut select, cursor);
            select.limit(u64::from(compiled.pagination.limit));
        }
    }

    let count = if compiled.pagination.count == CountPolicy::Emit {
        let mut count_select = SelectStatement::new();
        count_select
            .expr(Func::count(SeaExpr::col((E, TemporalEntities::Id))))
            .from(qualified(schema, TemporalEntities::Table).alias(E))
            .cond_where(entity_conditions)
            .cond_where(SeaExpr::exists(temporal_match_subquery(schema, compiled)));
        Some(count_select.take())
    } else {
        None
    };

    Ok(Stage1Query { select: select.take(), count })
}

/// Add the keyset predicate and ordering for one cursor direction over the
/// element dimension (clause 5.5.9.1).
///
/// The predicate and the ordering both read `temporal_entities.id` bare, which
/// is declared `COLLATE "C"`, so they share the byte-wise order of the implicit
/// `id` tiebreaker in either direction. A backward or from-end page is ordered
/// descending and reversed to ascending where the store reads the identifiers
/// back. The count statement is built separately and never carries the keyset,
/// so a counted cursor query still reports the whole collection's total.
fn apply_stage1_keyset(select: &mut SelectStatement, cursor: &PageCursor) {
    match cursor {
        PageCursor::After(id) => {
            select.and_where(SeaExpr::col((E, TemporalEntities::Id)).gt(SeaExpr::val(id.as_str())));
            select.order_by((E, TemporalEntities::Id), Order::Asc);
        }
        PageCursor::Before(id) => {
            select.and_where(SeaExpr::col((E, TemporalEntities::Id)).lt(SeaExpr::val(id.as_str())));
            select.order_by((E, TemporalEntities::Id), Order::Desc);
        }
        PageCursor::End => {
            select.order_by((E, TemporalEntities::Id), Order::Desc);
        }
    }
}

/// Build the correlated stream/history predicate shared by result and count.
fn temporal_match_subquery(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery) -> SelectStatement {
    let temporal_conditions = build_temporal_stage1_conditions(schema, compiled);
    let mut select = SelectStatement::new();
    select
        .expr(int_literal(1))
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(S))
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            SeaExpr::col((An, AttributeNames::AttributeNameKey)).equals((S, TemporalAttributeStreams::AttributeNameKey)),
        )
        .cond_where(SeaExpr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((E, TemporalEntities::TemporalEntityKey)))
        .cond_where(temporal_conditions);

    if compiled.window.interval.is_bounded() {
        select.join_as(
            JoinType::Join,
            qualified(schema, AttributeHistory::Table),
            Ah,
            SeaExpr::col((Ah, AttributeHistory::StreamKey)).equals((S, TemporalAttributeStreams::StreamKey)),
        );
    }
    select.take()
}

/// Build entity-level conditions for temporal queries (using "e" alias for
/// the `temporal_entities` table).
fn build_entity_conditions_for_temporal(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery, names: &QueryNames) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // Entity selectors, unioned (clause 5.2.33): `(sel1) OR (sel2) OR ...`,
    // each reached through the `e` alias stage 1 selects the temporal
    // catalogue under. Rendered only when every selector constrains — the
    // reading `EntitySelection::selectors_constrain` shares with the entity path.
    if compiled.entities.selectors_constrain() {
        let mut union = Condition::any();
        for selector in &compiled.entities.selectors {
            union = union.add(temporal_selector_condition(selector, &names.types));
        }
        condition = condition.add(union);
    }

    // Clause 5.5.14: "An EntityMap fixes the Entities to be considered." A
    // request narrowed to no entity at all cannot say so through a selector's
    // identifier list, where an empty list reads as no identifier constraint --
    // so it is conjoined here, where nothing can widen it back.
    match compiled.entities.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => condition = condition.add(false_literal()),
    }

    if let Some(q_expr) = compiled.entities.q_expr.as_ref() {
        condition = condition.add(q_filter::compile(q_expr, &InstanceScan::History(&windowed_instances(schema, compiled)), names));
    }

    if let Some(probe) = temporal_geo_existential(schema, compiled)? {
        condition = condition.add(SeaExpr::exists(probe));
    }

    // Clauses 5.7.4.5 and 6.3.13: an entity the `pick` / `omit` projection
    // would reduce to no member is not selected, so it is left out here, where
    // the count and the page are taken.
    if let Some(members) = projected_member_condition(schema, compiled) {
        condition = condition.add(members);
    }

    Ok(condition)
}

/// The Attribute instances a temporal `q` is checked against (clause 5.7.4.4
/// step S2): the entity's history rows valid within the window on the
/// requested Temporal Property, of every `datasetId`.
fn windowed_instances(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery) -> HistoryWindow {
    HistoryWindow::builder()
        .schema(schema.cloned())
        .temporal_entity_key(SeaExpr::col((E, TemporalEntities::TemporalEntityKey)))
        .axis(axis_column(compiled.window.timeproperty))
        .interval(compiled.window.interval)
        .build()
}

/// The correlated `EXISTS` over the `attribute_history` rows a temporal geo
/// query (clause 4.10) filters, restricted to the temporal window.
///
/// Clause 5.7.4.4 step S3: the geospatial restriction "shall be checked against
/// the `GeoProperty` instances that are within the interval defined by the
/// temporal query". Every instance of the target `GeoProperty` - the default
/// `location` and any named one alike - is an `attribute_history` row carrying a
/// `value_geometry`, so the probe reaches them uniformly through the stream rather
/// than through the single hoisted current-state `location` column, which holds
/// only one instant and answers a different question. Clause 4.10 makes one
/// in-window instance meeting the restriction enough, which is exactly this
/// existential. `None` when the query carries no geo restriction.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] or [`RenderError::Query`]
/// exactly as the shared relation rendering does.
fn temporal_geo_existential(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery) -> Result<Option<SelectStatement>, RenderError> {
    let Some(geo) = &compiled.entities.geo else {
        return Ok(None);
    };

    let planar = SeaExpr::col((Ahg, AttributeHistory::ValueGeometry));
    let geographic = cast(SeaExpr::col((Ahg, AttributeHistory::ValueGeometry)), PostgresType::Geography);
    let spatial = relation_expression(&geo.georel, planar, geographic, &geo.geometry)?;

    let mut probe = SelectStatement::new();
    probe
        .expr(int_literal(1))
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(Sg))
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            Ang,
            SeaExpr::col((Ang, AttributeNames::AttributeNameKey)).equals((Sg, TemporalAttributeStreams::AttributeNameKey)),
        )
        .join_as(
            JoinType::Join,
            qualified(schema, AttributeHistory::Table),
            Ahg,
            SeaExpr::col((Ahg, AttributeHistory::StreamKey)).equals((Sg, TemporalAttributeStreams::StreamKey)),
        )
        .cond_where(SeaExpr::col((Sg, TemporalAttributeStreams::TemporalEntityKey)).equals((E, TemporalEntities::TemporalEntityKey)))
        .cond_where(SeaExpr::col((Ang, AttributeNames::Name)).eq(geo.geoproperty.as_str()))
        .cond_where(SeaExpr::col((Ahg, AttributeHistory::ValueGeometry)).is_not_null())
        .cond_where(geo_window_condition(compiled))
        .cond_where(spatial);
    Ok(Some(probe.take()))
}

/// The temporal window as a predicate on the geo existential's own
/// `attribute_history` axis column.
///
/// The same interval the stream match applies (clause 5.2.21), restated against
/// the `ahg` alias so a `GeoProperty` instance counts only when it falls within
/// the window (clause 5.7.4.4 S3). The axis is the query's `timeproperty`,
/// defaulting to `observedAt` (clause 4.11).
fn geo_window_condition(compiled: &CompiledTemporalQuery) -> Condition {
    let axis = axis_column(compiled.window.timeproperty);
    within_interval(&SeaExpr::col((Ahg, axis)), &compiled.window.interval)
}

/// Render one selector's identifier, pattern and type predicates against the
/// `e` alias stage 1 selects `temporal_entities` under, `ANDed` (clause 5.2.33).
///
/// The entity path renders the same three axes against `entities`; the columns
/// carry the same names on both relations, so the two cannot drift on what a
/// selector means.
fn temporal_selector_condition(selector: &EntitySelector, types: &ResolvedNames<EntityTypeName>) -> Condition {
    let mut condition = Condition::all();
    if !selector.ids.is_empty() {
        let ids: Vec<&str> = selector.ids.iter().map(AsRef::as_ref).collect();
        condition = condition.add(SeaExpr::col((E, TemporalEntities::Id)).is_in(ids));
    }
    if let Some(pattern) = &selector.id_pattern {
        condition = condition.add(regex_match(SeaExpr::col((E, TemporalEntities::Id)), SeaExpr::val(pattern.as_str())));
    }
    if let Some(type_expr) = &selector.type_expr {
        condition = condition.add(type_condition(type_expr, &SeaExpr::col((E, TemporalEntities::EntityTypeNameKeys)), types));
    }
    condition
}

/// Build the clause 5.7.4.4 step S1 conditions of Stage 1 (the concerned
/// attributes and the window), plus the `scopeQ` of step S4.
fn build_temporal_stage1_conditions(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery) -> Condition {
    let mut condition = Condition::all();

    // Attribute filter: an.name IN (...).
    if !compiled.instances.attr_filter.is_empty() {
        let names: Vec<&str> = compiled.instances.attr_filter.iter().map(IriBuf::as_str).collect();
        condition = condition.add(SeaExpr::col((An, AttributeNames::Name)).is_in(names));
    }

    // Temporal filters (timerel + time_at), on the requested axis alone.
    let axis = axis_column(compiled.window.timeproperty);
    condition = condition.add(within_interval(&SeaExpr::col((Ah, axis)), &compiled.window.interval));

    // Historical scope filtering via EXISTS subquery against scope_history,
    // which hangs off the temporal entity grain rather than off `entities`.
    if let Some(scope_q) = &compiled.entities.scope_q {
        let target = ScopeTarget::history(
            (E, TemporalEntities::TemporalEntityKey),
            compiled.window.timeproperty,
            compiled.window.interval,
            schema.cloned(),
        );
        condition = condition.add(scope_condition(scope_q, &target));
    }

    condition
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        dataset::DatasetSelector,
        entity_selection::{EntitySelection, EntitySelector},
        filter::parser,
        geo::query::GeoQuery,
        ordering::{collation::IcuCollation, sort_direction::SortDirection},
        scope::query::ScopeQuery,
        temporal::{
            instance_selection::InstanceSelection,
            ordering::TemporalEntityOrdering,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
    use chrono::{DateTime, Utc};
    use sea_query::{PostgresQueryBuilder, Value, Values};
    use std::str::FromStr;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    fn ids(raw: &[&str]) -> Vec<Uri> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// The type dictionary with the one type the selector tests name.
    fn vehicle_is_known() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::default(),
            types: ResolvedNames::from_pairs([(
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".parse().expect("valid type name"),
                EntityTypeNameKey::from(3),
            )]),
        }
    }

    /// A `GeoJSON` point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: &serde_json::Value) -> Geometry {
        Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
    }

    /// A query with every filter absent, for tests that vary one part at a time.
    fn minimal_compiled() -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    /// The Stage 1 result statement, rendered with `$N` placeholders and its
    /// bound values, which is what the store hands to the driver.
    fn built(compiled: &CompiledTemporalQuery, names: &QueryNames) -> (String, Values) {
        build_stage1_entity_ids(None, compiled, names)
            .expect("valid temporal query")
            .select
            .build(PostgresQueryBuilder)
    }

    /// The same for the count statement, panicking when no count was requested.
    fn built_count(compiled: &CompiledTemporalQuery, names: &QueryNames) -> String {
        build_stage1_entity_ids(None, compiled, names)
            .expect("valid temporal query")
            .count
            .expect("count requested")
            .build(PostgresQueryBuilder)
            .0
    }

    fn has_timestamp(values: &Values) -> bool {
        values.0.iter().any(|value| matches!(value, Value::ChronoDateTimeUtc(_)))
    }

    #[test]
    fn build_entity_id_selection_with_type() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Vehicle"))))
                .build(),
        ];
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains("EXISTS(SELECT"), "{sql}");
        assert!(sql.contains("\"e\".\"entity_type_name_keys\" @> (ARRAY[3])"), "{sql}");
        assert!(sql.contains("JOIN \"attribute_names\""), "{sql}");
    }

    /// Clause 5.6.6 leaves the temporal evolution standing when the current
    /// entity is deleted, so the driving relation is the temporal catalogue and
    /// an unfiltered query names `entities` nowhere.
    #[test]
    fn the_statement_drives_from_the_temporal_catalogue_and_never_joins_entities() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("FROM \"temporal_entities\" AS \"e\""), "{sql}");
        assert!(sql.contains("\"temporal_attribute_streams\""), "{sql}");
        assert!(!sql.contains("\"entities\""), "{sql}");
        assert!(!sql.contains("temporal_entity_attributes"), "{sql}");
    }

    /// The stream catalogue carries the grain, so the correlation is on the
    /// surrogate entity key rather than on a repeated entity URI.
    #[test]
    fn the_stream_probe_correlates_on_the_temporal_entity_key() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"s\".\"temporal_entity_key\" = \"e\".\"temporal_entity_key\""), "{sql}");
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

    /// Clause 4.11 defaults the selector to `observedAt` and clause 5.2.21
    /// admits only the four semantic axes, so the window filters the requested
    /// column and nothing stands in for a missing value on it.
    #[test]
    fn each_time_property_filters_its_own_column_with_no_fallback() {
        let expected = [
            (TimeProp::Observed, "\"ah\".\"observed_at\""),
            (TimeProp::Created, "\"ah\".\"created_at\""),
            (TimeProp::Modified, "\"ah\".\"modified_at\""),
            (TimeProp::Deleted, "\"ah\".\"deleted_at\""),
        ];
        for (prop, column) in expected {
            let mut compiled = minimal_compiled();
            compiled.window.timeproperty = prop;
            compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
            let (sql, _) = built(&compiled, &QueryNames::default());
            assert!(sql.contains(column), "{prop}: {sql}");
            assert!(!sql.contains("COALESCE"), "{prop}: {sql}");
            assert!(!sql.contains("recorded_at"), "{prop}: {sql}");
        }
    }

    /// A bounded window is the only reason to read the hypertable, so an
    /// unbounded one stays on the catalogue.
    #[test]
    fn the_history_hypertable_is_joined_only_for_a_bounded_window() {
        let compiled = minimal_compiled();
        let (unbounded, _) = built(&compiled, &QueryNames::default());
        assert!(!unbounded.contains("attribute_history"), "{unbounded}");

        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let (bounded, _) = built(&compiled, &QueryNames::default());
        assert!(bounded.contains("\"ah\".\"stream_key\" = \"s\".\"stream_key\""), "{bounded}");
    }

    #[test]
    fn stage1_count_reuses_the_result_predicate_bind_order() {
        let mut compiled = minimal_compiled();
        compiled.pagination.count = CountPolicy::Emit;
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let (_, values) = built(&compiled, &QueryNames::default());
        let count_sql = built_count(&compiled, &QueryNames::default());

        assert!(count_sql.contains("EXISTS(SELECT"), "{count_sql}");
        assert!(count_sql.contains("$1"), "{count_sql}");
        assert!(count_sql.contains("$2"), "{count_sql}");
        assert!(has_timestamp(&values), "the window instant is bound: {values:?}");
    }

    #[test]
    fn stage1_includes_attribute_names_join() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("JOIN \"attribute_names\""), "{sql}");
    }

    #[test]
    fn stage1_includes_scope_q_filter() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid/Gardens").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("scope_history"), "SQL should reference scope_history table: {sql}");
        assert!(sql.contains("lquery"), "SQL should contain lquery cast: {sql}");
    }

    /// Scope history hangs off the temporal entity, so the probe correlates on
    /// the surrogate key the rebuilt relation carries.
    #[test]
    fn scope_history_correlates_on_the_temporal_entity_key() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"e\".\"temporal_entity_key\""), "{sql}");
    }

    #[test]
    fn stage1_scope_q_with_or_and_semantics() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A;/B,/C").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("AND"), "SQL should contain AND for semicolon-separated scopes: {sql}");
        assert!(sql.contains("OR"), "SQL should contain OR for comma-separated scopes: {sql}");
    }

    #[test]
    fn temporal_scope_q_between() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Between {
            from: instant("2024-01-01T00:00:00Z"),
            to: instant("2024-06-01T00:00:00Z"),
        };
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(">="), "Between should bound the scope probe from below: {sql}");
        assert!(sql.contains('<'), "Between should bound the scope probe from above: {sql}");
    }

    #[test]
    fn temporal_scope_q_compound() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A;/B,/C").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Before(instant("2024-06-01T00:00:00Z"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        // /A;/B -> AND, ,/C -> OR
        assert!(sql.contains("AND"), "Semicolon scopes produce AND: {sql}");
        assert!(sql.contains("OR"), "Comma scopes produce OR: {sql}");
        // Each scope term has its own EXISTS subquery, inside the stream
        // catalogue EXISTS predicate.
        let exists_count = sql.matches("EXISTS").count();
        pretty_assertions::assert_eq!(exists_count, 4, "The catalogue predicate wraps three scope predicates: {sql}");
    }

    /// The names the `q` tests below resolve: `speed` at key 7, `address` at 9.
    fn speed_and_address() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs([
                ("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameKey::from(7)),
                ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameKey::from(9)),
            ]),
            types: ResolvedNames::default(),
        }
    }

    /// Clause 5.7.4.4 S2: a temporal `q` "shall be checked against all the
    /// Attribute instances resulting from the initial filtering performed by the
    /// temporal query", so each Query Term probes the in-window history rows of
    /// its attribute, correlated to the temporal entity, and never current state.
    #[test]
    fn stage1_q_filter_probes_the_in_window_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">100").unwrap());
        let (sql, _) = built(&compiled, &speed_and_address());
        for fragment in [
            r#"FROM "temporal_attribute_streams" AS "qs" JOIN "attribute_history" AS "qh" ON "qh"."stream_key" = "qs"."stream_key""#,
            r#""qh"."deleted_at" IS NULL"#,
            r#""qh"."observed_at" >="#,
            r#""qh"."value_number" >"#,
            r#""qs"."temporal_entity_key" = "e"."temporal_entity_key""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        assert!(!sql.contains("current_attribute_values"), "a temporal q never reads current state: {sql}");
        assert!(!sql.contains(r#""entities""#), "{sql}");
    }

    /// The window a `q` term reads is on the requested Temporal Property.
    #[test]
    fn stage1_q_filter_reads_the_window_on_the_requested_timeproperty() {
        let mut compiled = minimal_compiled();
        compiled.window.timeproperty = TimeProp::Modified;
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\"==1").unwrap());
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(sql.contains(r#""qh"."modified_at" <"#), "{sql}");
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
            compiled.pagination.count = CountPolicy::Emit;
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
            assert!(!sql.contains("dataset_id"), "{selectors:?}: {sql}");
            let count = built_count(&compiled, &speed_and_address());
            assert!(!count.contains("dataset_id"), "{selectors:?}: {count}");
        }
    }

    /// Each Query Term is its own existential, combined by the q tree.
    #[test]
    fn each_term_of_an_or_is_its_own_windowed_existential() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">100|\"https://example.com/speed\"<10").unwrap());
        let (sql, _) = built(&compiled, &speed_and_address());
        pretty_assertions::assert_eq!(sql.matches(r#"AS "qh""#).count(), 2, "{sql}");
        assert!(sql.contains(" OR EXISTS"), "{sql}");
    }

    #[test]
    fn stage1_q_filter_on_an_unknown_attribute_is_unsatisfiable() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/never-written\">50").unwrap());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("FALSE"), "{sql}");
    }

    /// A compound-value term is answered by the payload jsonpath over each
    /// in-window instance rebuilt as a one-instance entity document, inside the
    /// history probe.
    #[test]
    fn a_q_predicate_on_a_compound_value_reads_the_rebuilt_history_instance() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/address\"[city]==\"Berlin\"").unwrap());
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(
            sql.contains("jsonb_path_query_first(jsonb_build_array(jsonb_build_object('https://example.com/address'"),
            "{sql}"
        );
        assert!(!sql.contains("entity_documents"), "{sql}");
    }

    /// Clause 5.7.4.4 S3: the geospatial restriction is checked against the
    /// `GeoProperty` instances "within the interval defined by the temporal
    /// query", which are `attribute_history` rows -- not the single hoisted
    /// current-state `location` column, which holds one instant and answers a
    /// different question. The default `location` reaches its history rows
    /// through the stream just as any named `GeoProperty` does.
    #[test]
    fn a_default_location_geo_query_reads_the_windowed_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: Geometry::from_geojson_value(&serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"FROM "temporal_attribute_streams" AS "sg""#), "{sql}");
        assert!(sql.contains(r#"JOIN "attribute_history" AS "ahg""#), "{sql}");
        assert!(sql.contains(r#""ahg"."value_geometry""#), "{sql}");
        assert!(
            sql.contains(r#""sg"."temporal_entity_key" = "e"."temporal_entity_key""#),
            "correlated to the temporal entity: {sql}"
        );
        assert!(!sql.contains("entity_search"), "the geo restriction no longer reads current state: {sql}");
    }

    /// Clause 5.7.4.4 S3 restricts the geo instances to the temporal window, so
    /// the geo existential restates the window predicate on its own
    /// `attribute_history` axis rather than trusting the stream match alone.
    #[test]
    fn a_temporal_geo_query_restricts_the_geo_instances_to_the_window() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2020-07-01T12:05:00Z"));
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("2000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([-8.503, 41.202])),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#""ahg"."observed_at""#), "the window is applied to the geo instances: {sql}");
        assert!(sql.contains("ST_DWithin"), "near renders a distance relation on the geo instances: {sql}");
    }

    #[test]
    fn no_scope_q_unchanged() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(!sql.contains("scope_history"), "No scope_history reference without scopeQ: {sql}");
    }

    /// Clause 5.7.4.4 S3 applies to a named `GeoProperty` exactly as to the
    /// default `location`: its instances are the `attribute_history` rows of the
    /// stream carrying that name, restricted to the window, so a named property
    /// reaches the same history rows the default one does rather than the
    /// current-state projection.
    #[test]
    fn a_temporal_geo_query_on_a_named_geoproperty_reads_the_windowed_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/customGeo".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"FROM "temporal_attribute_streams" AS "sg""#), "{sql}");
        assert!(
            sql.contains(r#"JOIN "attribute_names" AS "ang""#),
            "the target GeoProperty is named through the dictionary: {sql}"
        );
        assert!(sql.contains(r#""ahg"."value_geometry""#), "{sql}");
        assert!(
            !sql.contains("current_attribute_values"),
            "a temporal geo query reads history, not the current-state projection: {sql}"
        );
    }

    /// The IRI is reached by dictionary key, so an apostrophe in one never
    /// enters the statement text.
    #[test]
    fn a_temporal_geoproperty_iri_never_reaches_the_statement_text() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/o'brien".parse().expect("valid attribute name"),
        });
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([(
                "https://example.com/o'brien".parse().expect("valid attribute name"),
                AttributeNameKey::from(9_i32),
            )]),
            types: ResolvedNames::default(),
        };
        let (sql, _) = built(&compiled, &names);
        assert!(!sql.contains("brien"), "{sql}");
        // Every literal contributes a balanced pair of quotes, so the total is
        // even unless a literal was terminated early.
        assert_eq!(sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {sql}");
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
        compiled.pagination.count = CountPolicy::Emit;
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Ascending).build());
        let sql = built_count(&compiled, &QueryNames::default());
        assert!(!sql.contains("ORDER BY"), "{sql}");
    }

    fn boundary(id: &str) -> Uri {
        id.parse().expect("valid URI")
    }

    /// Clause 5.5.9.1: a forward cursor pages the element dimension past the
    /// boundary id over the default order -- `WHERE id > $x ORDER BY id ASC` and
    /// no `OFFSET`.
    #[test]
    fn stage1_keyset_after_pages_forward_ascending_without_offset() {
        let mut compiled = minimal_compiled();
        compiled.keyset = Some(PageCursor::After(boundary("urn:ngsi-ld:Vehicle:001")));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#""e"."id" > $"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" ASC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "a keyset page never skips: {sql}");
    }

    /// Clause 5.5.9.1: a backward cursor pages before the boundary over the
    /// reversed order -- `WHERE id < $x ORDER BY id DESC` -- which the store
    /// reverses to ascending after fetching.
    #[test]
    fn stage1_keyset_before_pages_backward_descending_without_offset() {
        let mut compiled = minimal_compiled();
        compiled.keyset = Some(PageCursor::Before(boundary("urn:ngsi-ld:Vehicle:042")));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#""e"."id" < $"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" DESC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// Clause 5.5.9.1: the from-end cursor takes the final page -- `ORDER BY id
    /// DESC`, no boundary predicate -- which the store reverses to ascending.
    #[test]
    fn stage1_keyset_end_takes_the_final_page_descending() {
        let mut compiled = minimal_compiled();
        compiled.keyset = Some(PageCursor::End);
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"ORDER BY "e"."id" DESC"#), "{sql}");
        assert!(!sql.contains(r#""e"."id" >"#) && !sql.contains(r#""e"."id" <"#), "no boundary predicate: {sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// The count statement ignores the keyset, so a counted cursor page still
    /// reports the whole collection's total rather than the remaining tail.
    #[test]
    fn a_keyset_count_still_counts_the_whole_collection() {
        let mut compiled = minimal_compiled();
        compiled.pagination.count = CountPolicy::Emit;
        compiled.keyset = Some(PageCursor::After(boundary("urn:ngsi-ld:Vehicle:001")));
        let sql = built_count(&compiled, &QueryNames::default());
        assert!(!sql.contains(r#""e"."id" >"#), "the count is not narrowed by the cursor: {sql}");
    }
}
